//! Language logos as file icons (from Devicon, MIT licence; the logos are
//! trademarks of their owners, see docs/THIRD-PARTY.md).
//!
//! Colourful logos are drawn in their own colours ([`LogoStyle::Colour`],
//! rasterised by [`raster`]). Logos that are black or very dark would vanish
//! on a dark theme, so they are drawn as a monochrome shape in the theme's
//! text colour ([`LogoStyle::Tinted`]), like the plain icons.

mod raster;

use gpui::{
    App, Hsla, ImageSource, IntoElement, Pixels, RenderOnce, SharedString, Styled, Window, px, svg,
};

use crate::{IconName, icon, theme};

pub use raster::logo_rasterizations;

/// How a logo is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogoStyle {
    /// In its own colours.
    Colour,
    /// As a monochrome shape in the theme's text colour (black or very
    /// dark logos).
    Tinted,
}

macro_rules! logos {
    ($($variant:ident => $file:literal, $style:ident, $name:literal;)*) => {
        /// A logo shipped in `assets/file-icons/`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum FileLogo {
            $(#[doc = $name] $variant,)*
        }

        impl FileLogo {
            /// Every logo.
            pub const ALL: &'static [FileLogo] = &[$(FileLogo::$variant),*];

            /// Asset path served by [`crate::Assets`].
            pub fn path(self) -> &'static str {
                match self {
                    $(FileLogo::$variant => concat!("file-icons/", $file, ".svg"),)*
                }
            }

            /// How it is drawn.
            pub fn style(self) -> LogoStyle {
                match self {
                    $(FileLogo::$variant => LogoStyle::$style,)*
                }
            }

            /// What it stands for, as people name it.
            pub fn name(self) -> &'static str {
                match self {
                    $(FileLogo::$variant => $name,)*
                }
            }
        }

        /// The logo files, embedded like the other assets.
        pub(crate) static LOGO_FILES: &[(&str, &[u8])] = &[
            $((
                concat!("file-icons/", $file, ".svg"),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"), "/../../assets/file-icons/", $file, ".svg"
                )),
            ),)*
        ];
    };
}

logos! {
    Bash => "bash", Tinted, "Bash";
    C => "c", Colour, "C";
    Clojure => "clojure", Colour, "Clojure";
    Cmake => "cmake", Colour, "CMake";
    Cplusplus => "cplusplus", Colour, "C++";
    Csharp => "csharp", Colour, "C#";
    Css => "css3", Colour, "CSS";
    Dart => "dart", Colour, "Dart";
    Docker => "docker", Colour, "Docker";
    Elixir => "elixir", Colour, "Elixir";
    Erlang => "erlang", Colour, "Erlang";
    Fsharp => "fsharp", Colour, "F#";
    Git => "git", Colour, "Git";
    Go => "go", Colour, "Go";
    Gradle => "gradle", Tinted, "Gradle";
    Graphql => "graphql", Colour, "GraphQL";
    Haskell => "haskell", Colour, "Haskell";
    Html => "html5", Colour, "HTML";
    Java => "java", Colour, "Java";
    Javascript => "javascript", Colour, "JavaScript";
    Json => "json", Tinted, "JSON";
    Julia => "julia", Colour, "Julia";
    Kotlin => "kotlin", Colour, "Kotlin";
    Lua => "lua", Tinted, "Lua";
    Markdown => "markdown", Tinted, "Markdown";
    Nix => "nixos", Colour, "Nix";
    Nodejs => "nodejs", Colour, "Node.js";
    Npm => "npm", Colour, "npm";
    Ocaml => "ocaml", Colour, "OCaml";
    Perl => "perl", Tinted, "Perl";
    Php => "php", Colour, "PHP";
    Powershell => "powershell", Tinted, "PowerShell";
    Python => "python", Colour, "Python";
    R => "r", Colour, "R";
    React => "react", Colour, "React";
    Ruby => "ruby", Colour, "Ruby";
    Rust => "rust", Tinted, "Rust";
    Sass => "sass", Colour, "Sass";
    Scala => "scala", Colour, "Scala";
    Svelte => "svelte", Colour, "Svelte";
    Swift => "swift", Colour, "Swift";
    Terraform => "terraform", Colour, "Terraform";
    Typescript => "typescript", Colour, "TypeScript";
    Vim => "vim", Colour, "Vim script";
    Vue => "vuejs", Colour, "Vue";
    Wasm => "wasm", Colour, "WebAssembly";
    Yaml => "yaml", Tinted, "YAML";
    Zig => "zig", Colour, "Zig";
}

/// What a file icon shows: a plain icon or a language logo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileGlyph {
    /// A monochrome icon, tinted.
    Icon(IconName),
    /// A language logo.
    Logo(FileLogo),
}

impl From<IconName> for FileGlyph {
    fn from(name: IconName) -> Self {
        Self::Icon(name)
    }
}

impl From<FileLogo> for FileGlyph {
    fn from(logo: FileLogo) -> Self {
        Self::Logo(logo)
    }
}

/// A file icon element: 16 px by default. Plain icons and tinted logos take
/// [`FileIcon::color`] (plain icons default to `text_muted`, tinted logos to
/// `text`); colour logos keep their own colours.
#[derive(IntoElement)]
pub struct FileIcon {
    glyph: FileGlyph,
    size: Pixels,
    color: Option<Hsla>,
}

/// Creates a [`FileIcon`].
pub fn file_glyph(glyph: impl Into<FileGlyph>) -> FileIcon {
    FileIcon {
        glyph: glyph.into(),
        size: px(16.),
        color: None,
    }
}

impl FileIcon {
    /// Side length.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Tint of a plain icon or a tinted logo.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// What it shows.
    pub fn glyph(&self) -> FileGlyph {
        self.glyph
    }

    /// The tint it was given, if any.
    pub fn tint(&self) -> Option<Hsla> {
        self.color
    }
}

impl RenderOnce for FileIcon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &theme(cx).colors;
        let plain = |name: IconName, colors: &crate::ThemeColors| {
            icon(name)
                .size(self.size)
                .color(self.color.unwrap_or(colors.text_muted))
                .into_any_element()
        };
        match self.glyph {
            FileGlyph::Icon(name) => plain(name, colors),
            FileGlyph::Logo(logo) if logo.style() == LogoStyle::Tinted => svg()
                .path(SharedString::new_static(logo.path()))
                .size(self.size)
                .flex_none()
                .text_color(self.color.unwrap_or(colors.text))
                .into_any_element(),
            FileGlyph::Logo(logo) => {
                let colors = colors.clone();
                let side = raster::device_px(self.size, window.scale_factor());
                match raster::logo_image(logo, side, cx) {
                    Some(image) => gpui::img(ImageSource::Render(image))
                        .size(self.size)
                        .flex_none()
                        .into_any_element(),
                    None => plain(IconName::File, &colors),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
