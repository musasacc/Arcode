//! Discovery and the registry of known plugins (docs/PLUGINS.md §3).
//!
//! At launch only `plugin.toml` files are read: no wasm, no engine. The
//! registry answers "which plugin does this command / panel / opener / lens
//! language belong to", which is all the app needs until an activation.
//!
//! Discovery order is bundled < installed < dev: a later source replaces an
//! earlier plugin with the same id (the replaced one is kept in
//! [`RegisteredPlugin::shadowed`] for the Plugins view).

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::manifest::{
    ActivationEvent, CommandContribution, EffectiveLimits, LanguageContribution,
    MAX_MANIFEST_BYTES, Manifest, ManifestError, PanelContribution, PluginId, PluginSource,
    QualifiedId, UserLimits,
};

/// Where to look for plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryRoots {
    /// Directories holding `<id>/plugin.toml` folders shipped with Arcode.
    pub bundled: Vec<PathBuf>,
    /// Directories holding user-installed `<id>/plugin.toml` folders.
    pub installed: Vec<PathBuf>,
    /// Plugin folders loaded unpacked for this session (each is one plugin).
    pub dev: Vec<PathBuf>,
}

/// A plugin known to the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredPlugin {
    /// The validated manifest.
    pub manifest: Manifest,
    /// The plugin folder.
    pub dir: PathBuf,
    /// Where it was found.
    pub source: PluginSource,
    /// Limits after clamping to the user's maxima.
    pub limits: EffectiveLimits,
    /// Earlier plugins with the same id that this one replaced.
    pub shadowed: Vec<(PluginSource, PathBuf)>,
}

impl RegisteredPlugin {
    /// The plugin id.
    pub fn id(&self) -> &PluginId {
        self.manifest.id()
    }

    /// Path of the component file.
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.plugin.entry)
    }
}

/// A folder that was skipped, with the reason (shown in the Plugins view).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryProblem {
    /// The plugin folder.
    pub dir: PathBuf,
    /// Which root it came from.
    pub source: PluginSource,
    /// Why it was skipped.
    pub error: ManifestError,
}

/// Opener patterns of one contribution, split by what they match.
#[derive(Debug, Clone)]
struct OpenerIndex {
    id: QualifiedId,
    by_name: GlobSet,
    by_path: GlobSet,
}

/// All plugins found by [`Registry::discover`], keyed by id.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    plugins: BTreeMap<PluginId, RegisteredPlugin>,
    problems: Vec<DiscoveryProblem>,
    openers: Vec<OpenerIndex>,
}

impl Registry {
    /// Reads every `plugin.toml` under `roots`. Never fails: unreadable or
    /// invalid folders become [`DiscoveryProblem`]s.
    pub fn discover(roots: &DiscoveryRoots, user: &UserLimits) -> Self {
        let mut found = Vec::new();
        let mut problems = Vec::new();
        for (source, dirs) in [
            (PluginSource::Bundled, &roots.bundled),
            (PluginSource::Installed, &roots.installed),
        ] {
            for root in dirs {
                for dir in plugin_folders(root) {
                    match load(&dir, source) {
                        Ok(manifest) => found.push((manifest, dir, source)),
                        Err(error) => problems.push(DiscoveryProblem { dir, source, error }),
                    }
                }
            }
        }
        for dir in &roots.dev {
            match load(dir, PluginSource::Dev) {
                Ok(manifest) => found.push((manifest, dir.clone(), PluginSource::Dev)),
                Err(error) => problems.push(DiscoveryProblem {
                    dir: dir.clone(),
                    source: PluginSource::Dev,
                    error,
                }),
            }
        }
        let mut registry = Self::from_manifests(found, user);
        registry.problems.splice(0..0, problems);
        registry
    }

    /// Builds a registry from already-parsed manifests, in discovery order
    /// (later entries replace earlier ones with the same id).
    pub fn from_manifests(
        found: impl IntoIterator<Item = (Manifest, PathBuf, PluginSource)>,
        user: &UserLimits,
    ) -> Self {
        let mut plugins: BTreeMap<PluginId, RegisteredPlugin> = BTreeMap::new();
        for (manifest, dir, source) in found {
            let limits = manifest.limits.clamp(user);
            let shadowed = match plugins.remove(manifest.id()) {
                Some(old) => {
                    let mut chain = old.shadowed;
                    chain.push((old.source, old.dir));
                    chain
                }
                None => Vec::new(),
            };
            plugins.insert(
                manifest.id().clone(),
                RegisteredPlugin {
                    manifest,
                    dir,
                    source,
                    limits,
                    shadowed,
                },
            );
        }
        let mut problems = Vec::new();
        let openers = plugins
            .values()
            .flat_map(|p| opener_indexes(p, &mut problems))
            .collect();
        Self {
            plugins,
            problems,
            openers,
        }
    }

    /// A plugin by id.
    pub fn get(&self, id: &PluginId) -> Option<&RegisteredPlugin> {
        self.plugins.get(id)
    }

    /// Every plugin, ordered by id.
    pub fn plugins(&self) -> impl Iterator<Item = &RegisteredPlugin> {
        self.plugins.values()
    }

    /// Folders that were skipped during discovery.
    pub fn problems(&self) -> &[DiscoveryProblem] {
        &self.problems
    }

    /// Every contributed command with its namespaced id.
    pub fn commands(
        &self,
    ) -> impl Iterator<Item = (QualifiedId, &RegisteredPlugin, &CommandContribution)> {
        self.plugins.values().flat_map(|p| {
            p.manifest
                .contributes
                .commands
                .iter()
                .map(move |c| (p.id().qualify(&c.id), p, c))
        })
    }

    /// The plugin and contribution behind a namespaced command id.
    pub fn command(&self, id: &QualifiedId) -> Option<(&RegisteredPlugin, &CommandContribution)> {
        let plugin = self.plugin_of(id)?;
        let cmd = plugin
            .manifest
            .contributes
            .commands
            .iter()
            .find(|c| c.id == id.local())?;
        Some((plugin, cmd))
    }

    /// The plugin and contribution behind a namespaced panel id.
    pub fn panel(&self, id: &QualifiedId) -> Option<(&RegisteredPlugin, &PanelContribution)> {
        let plugin = self.plugin_of(id)?;
        let panel = plugin
            .manifest
            .contributes
            .panels
            .iter()
            .find(|p| p.id == id.local())?;
        Some((plugin, panel))
    }

    /// Every contributed language with its plugin.
    pub fn languages(&self) -> impl Iterator<Item = (&RegisteredPlugin, &LanguageContribution)> {
        self.plugins
            .values()
            .flat_map(|p| p.manifest.contributes.languages.iter().map(move |l| (p, l)))
    }

    /// Plugins that provide code lenses for `language`.
    pub fn lens_providers(&self, language: &str) -> Vec<&RegisteredPlugin> {
        let event = ActivationEvent::LensLanguage(language.to_owned());
        self.plugins_for(&event)
    }

    /// The opener for a file, given its project-relative path. Patterns
    /// without `/` match the file name; the first plugin by id wins.
    pub fn opener_for(&self, relative_path: &Path) -> Option<&QualifiedId> {
        let name = relative_path.file_name()?;
        let unix_path = relative_path.to_string_lossy().replace('\\', "/");
        self.openers
            .iter()
            .find(|o| o.by_name.is_match(name) || o.by_path.is_match(&unix_path))
            .map(|o| &o.id)
    }

    /// Plugins that `event` activates.
    pub fn plugins_for(&self, event: &ActivationEvent) -> Vec<&RegisteredPlugin> {
        self.plugins
            .values()
            .filter(|p| p.manifest.activation_events().contains(event))
            .collect()
    }

    fn plugin_of(&self, id: &QualifiedId) -> Option<&RegisteredPlugin> {
        self.plugins.get(&PluginId::parse(id.plugin()).ok()?)
    }
}

fn opener_indexes(
    plugin: &RegisteredPlugin,
    problems: &mut Vec<DiscoveryProblem>,
) -> Vec<OpenerIndex> {
    let mut out = Vec::new();
    for opener in &plugin.manifest.contributes.openers {
        let (mut by_name, mut by_path) = (GlobSetBuilder::new(), GlobSetBuilder::new());
        for pattern in &opener.patterns {
            if pattern.contains('/') {
                // Validated in the manifest; `*` must not cross directories here.
                if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() {
                    by_path.add(glob);
                }
            } else if let Ok(glob) = Glob::new(pattern) {
                by_name.add(glob);
            }
        }
        match (by_name.build(), by_path.build()) {
            (Ok(by_name), Ok(by_path)) => out.push(OpenerIndex {
                id: plugin.id().qualify(&opener.id),
                by_name,
                by_path,
            }),
            (Err(e), _) | (_, Err(e)) => problems.push(DiscoveryProblem {
                dir: plugin.dir.clone(),
                source: plugin.source,
                error: ManifestError::Invalid(format!("opener `{}`: {e}", opener.id)),
            }),
        }
    }
    out
}

/// Sub-folders of `root`, sorted by name. A missing root is simply empty.
fn plugin_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

/// Reads and validates `<dir>/plugin.toml`.
///
/// Bundled and installed plugins must be regular files (no symlinks) in a
/// folder named after the plugin id; dev folders may be anywhere.
pub fn load(dir: &Path, source: PluginSource) -> Result<Manifest, ManifestError> {
    let path = dir.join("plugin.toml");
    let meta = fs::symlink_metadata(&path).map_err(|e| ManifestError::Io(e.to_string()))?;
    let regular = meta.file_type().is_file();
    if !regular && (source != PluginSource::Dev || !path.is_file()) {
        return Err(ManifestError::Io(
            "plugin.toml is not a regular file".into(),
        ));
    }
    let mut text = String::new();
    let file = fs::File::open(&path).map_err(|e| ManifestError::Io(e.to_string()))?;
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| ManifestError::Io(e.to_string()))?;
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ManifestError::Io(format!(
            "plugin.toml is larger than {} KiB",
            MAX_MANIFEST_BYTES / 1024
        )));
    }
    let manifest = Manifest::parse(&text, source)?;
    if source != PluginSource::Dev {
        let folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if folder != manifest.id().as_str() {
            return Err(ManifestError::Invalid(format!(
                "folder `{folder}` does not match plugin id `{}`",
                manifest.id()
            )));
        }
    }
    Ok(manifest)
}
