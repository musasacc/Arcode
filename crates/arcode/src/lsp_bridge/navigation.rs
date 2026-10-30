//! Navigation requests to the language servers of a document: go to
//! definition / type definition / implementation, find references and
//! document / workspace symbols. Each returns `None` right away when no
//! running server offers the feature, so the caller can fall back (e.g.
//! to the tree-sitter outline); otherwise a runtime-agnostic future.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use arcode_buffer::Point;
use arcode_editor::Editor;
use arcode_lsp::lsp_types::request::{
    DocumentSymbolRequest, GotoDefinition, GotoImplementation, GotoTypeDefinition, References,
    WorkspaceSymbolRequest,
};
use arcode_lsp::lsp_types::{
    DocumentSymbolParams, GotoDefinitionParams, GotoDefinitionResponse,
    ImplementationProviderCapability, OneOf, Position, ReferenceContext, ReferenceParams,
    ServerCapabilities, TextDocumentIdentifier, TextDocumentPositionParams,
    TypeDefinitionProviderCapability, WorkspaceSymbolParams,
};
use arcode_lsp::{LspManager, PositionEncoding, path_to_uri};

use super::LspBridge;
use super::convert::buffer_encoding;
use super::nav_convert::{
    document_symbol_items, goto_targets, reference_items, workspace_symbol_items,
};
use crate::navigation::item::{NavItem, NavTarget};

/// A boxed request future.
pub type NavFuture<T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'static>>;

/// Which kind of "go to" request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GotoKind {
    /// `textDocument/definition`
    Definition,
    /// `textDocument/typeDefinition`
    TypeDefinition,
    /// `textDocument/implementation`
    Implementation,
}

impl GotoKind {
    /// What the target is called in messages ("definition").
    pub fn noun(self) -> &'static str {
        match self {
            GotoKind::Definition => "definition",
            GotoKind::TypeDefinition => "type definition",
            GotoKind::Implementation => "implementation",
        }
    }
}

/// Whether an `Option<OneOf<bool, _>>` capability is on.
pub(crate) fn one_of<T>(cap: &Option<OneOf<bool, T>>) -> bool {
    matches!(cap, Some(OneOf::Left(true)) | Some(OneOf::Right(_)))
}

fn supports_goto(kind: GotoKind, c: &ServerCapabilities) -> bool {
    match kind {
        GotoKind::Definition => one_of(&c.definition_provider),
        GotoKind::TypeDefinition => matches!(
            c.type_definition_provider,
            Some(TypeDefinitionProviderCapability::Simple(true))
                | Some(TypeDefinitionProviderCapability::Options(_))
        ),
        GotoKind::Implementation => matches!(
            c.implementation_provider,
            Some(ImplementationProviderCapability::Simple(true))
                | Some(ImplementationProviderCapability::Options(_))
        ),
    }
}

/// The text document position of `at` in `editor` for `encoding`.
pub(crate) fn text_position(
    editor: &Editor,
    path: &Path,
    at: Point,
    encoding: PositionEncoding,
) -> TextDocumentPositionParams {
    let (line, character) = editor.point_to_lsp(at, buffer_encoding(encoding));
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: path_to_uri(path),
        },
        position: Position::new(line, character),
    }
}

impl LspBridge {
    /// The manager and the document path when a server runs for `editor`.
    pub(crate) fn served(&self, editor: &Editor) -> Option<(Arc<LspManager>, PathBuf)> {
        let manager = self.manager.clone()?;
        let path = editor.path()?.to_path_buf();
        (!manager.running_servers(&path).is_empty()).then_some((manager, path))
    }

    /// Whether a running server for `editor` passes `supports`.
    fn offers(&self, editor: &Editor, supports: impl Fn(&ServerCapabilities) -> bool) -> bool {
        self.served(editor).is_some_and(|(manager, path)| {
            manager
                .running_servers(&path)
                .iter()
                .filter_map(|s| manager.capabilities(&path, s))
                .any(|c| supports(&c))
        })
    }

    /// Raw definition-style answer for `at` with the answering server's
    /// encoding (also used for the Ctrl+hover link).
    pub(crate) fn goto_raw(
        &self,
        editor: &Editor,
        at: Point,
        kind: GotoKind,
    ) -> Option<NavFuture<(Option<GotoDefinitionResponse>, PositionEncoding)>> {
        if !self.offers(editor, |c| supports_goto(kind, c)) {
            return None;
        }
        let (manager, path) = self.served(editor)?;
        let params = |encoding| GotoDefinitionParams {
            text_document_position_params: text_position(editor, &path, at, encoding),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let supports = move |c: &ServerCapabilities| supports_goto(kind, c);
        let request: Pin<Box<dyn Future<Output = _> + Send>> = match kind {
            GotoKind::Definition => {
                Box::pin(manager.request_with::<GotoDefinition>(&path, supports, params))
            }
            GotoKind::TypeDefinition => {
                Box::pin(manager.request_with::<GotoTypeDefinition>(&path, supports, params))
            }
            GotoKind::Implementation => {
                Box::pin(manager.request_with::<GotoImplementation>(&path, supports, params))
            }
        };
        Some(Box::pin(async move {
            let (server, response) = request.await?;
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            Ok((response, encoding))
        }))
    }

    /// The targets of a definition, type definition or implementation
    /// request at `at`.
    pub fn goto(
        &self,
        editor: &Editor,
        at: Point,
        kind: GotoKind,
    ) -> Option<NavFuture<Vec<NavTarget>>> {
        let raw = self.goto_raw(editor, at, kind)?;
        Some(Box::pin(async move {
            let (response, encoding) = raw.await?;
            Ok(goto_targets(response, encoding))
        }))
    }

    /// Every reference to the symbol at `at`, declaration included.
    pub fn references(&self, editor: &Editor, at: Point) -> Option<NavFuture<Vec<NavItem>>> {
        if !self.offers(editor, |c| one_of(&c.references_provider)) {
            return None;
        }
        let (manager, path) = self.served(editor)?;
        let request = manager.request_with::<References>(
            &path,
            |c| one_of(&c.references_provider),
            |encoding| ReferenceParams {
                text_document_position: text_position(editor, &path, at, encoding),
                context: ReferenceContext {
                    include_declaration: true,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        Some(Box::pin(async move {
            let (server, locations) = request.await?;
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            Ok(reference_items(locations.unwrap_or_default(), encoding))
        }))
    }

    /// The document's symbols from its server.
    pub fn document_symbols(&self, editor: &Editor) -> Option<NavFuture<Vec<NavItem>>> {
        if !self.offers(editor, |c| one_of(&c.document_symbol_provider)) {
            return None;
        }
        let (manager, path) = self.served(editor)?;
        let request = manager.request_with::<DocumentSymbolRequest>(
            &path,
            |c| one_of(&c.document_symbol_provider),
            |_| DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: path_to_uri(&path),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        Some(Box::pin(async move {
            let (server, response) = request.await?;
            let encoding = manager
                .position_encoding(&path, &server)
                .unwrap_or_default();
            Ok(document_symbol_items(response, &path, encoding))
        }))
    }

    /// The open document whose servers answer workspace symbol queries:
    /// `editor`'s, else any open document with such a server.
    pub fn workspace_symbol_path(&self, editor: Option<&Editor>) -> Option<PathBuf> {
        let manager = self.manager.as_ref()?;
        let offers = |path: &Path| {
            manager
                .running_servers(path)
                .iter()
                .filter_map(|s| manager.capabilities(path, s))
                .any(|c| one_of(&c.workspace_symbol_provider))
        };
        editor
            .and_then(|e| e.path().map(Path::to_path_buf))
            .filter(|p| offers(p))
            .or_else(|| {
                self.docs
                    .values()
                    .filter(|d| d.synced && d.editor.upgrade().is_some())
                    .map(|d| d.path.clone())
                    .find(|p| offers(p))
            })
    }

    /// Workspace symbols matching `query`, asked through the servers of
    /// [`LspBridge::workspace_symbol_path`].
    pub fn workspace_symbols(
        &self,
        editor: Option<&Editor>,
        query: String,
    ) -> Option<NavFuture<Vec<NavItem>>> {
        let manager = self.manager.clone()?;
        let path = self.workspace_symbol_path(editor)?;
        let request = manager.request_all_with::<WorkspaceSymbolRequest>(
            &path,
            |c| one_of(&c.workspace_symbol_provider),
            |_| WorkspaceSymbolParams {
                query: query.clone(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        Some(Box::pin(async move {
            let mut items = Vec::new();
            for (server, response) in request.await {
                let encoding = manager
                    .position_encoding(&path, &server)
                    .unwrap_or_default();
                items.extend(workspace_symbol_items(response, encoding));
            }
            Ok(items)
        }))
    }
}
