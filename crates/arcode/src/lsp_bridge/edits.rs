//! Workspace edits on their way to the workspace: from a code action or
//! rename the editor asked for, or from a server's `workspace/applyEdit`.
//! The bridge plans them ([`super::workspace_edit::plan`]) and emits
//! [`LspBridgeEvent::ApplyEdit`]; the workspace applies them and answers
//! through the request's [`EditReply`].

use std::sync::Arc;

use arcode_lsp::lsp_types::WorkspaceEdit;
use arcode_lsp::{EditOutcome, EditReply, PositionEncoding};
use gpui::Context;

use super::workspace_edit::{FileOp, plan};
use super::{LspBridge, LspBridgeEvent};

/// A planned workspace edit.
#[derive(Debug)]
pub struct EditRequest {
    /// The operations, in order.
    pub ops: Vec<FileOp>,
    /// Encoding of the text edits' positions.
    pub encoding: PositionEncoding,
    /// What to call it in messages ("Rename", a code action's title).
    pub label: Option<String>,
    /// Where the outcome goes.
    pub reply: EditReply,
}

/// A shared [`EditRequest`] (events must be `Clone` and comparable).
#[derive(Debug, Clone)]
pub struct EditHandle(pub Arc<EditRequest>);

impl PartialEq for EditHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for EditHandle {}

/// Awaits the workspace's answer; an edit nobody applied is an error.
pub async fn edit_applied(
    outcome: impl std::future::Future<Output = Result<EditOutcome, impl std::fmt::Debug>>,
) -> anyhow::Result<()> {
    match outcome.await {
        Ok(EditOutcome { applied: true, .. }) => Ok(()),
        Ok(EditOutcome { failure_reason, .. }) => Err(anyhow::anyhow!(
            failure_reason.unwrap_or_else(|| "the edit was not applied".into())
        )),
        Err(_) => Err(anyhow::anyhow!("no workspace applied the edit")),
    }
}

impl LspBridge {
    /// Plans `edit` and hands it to the workspace; answers `reply` right
    /// away when it cannot be planned.
    pub(crate) fn apply_workspace_edit(
        &mut self,
        edit: WorkspaceEdit,
        encoding: PositionEncoding,
        label: Option<String>,
        reply: EditReply,
        cx: &mut Context<Self>,
    ) {
        let ops = match plan(edit) {
            Ok(ops) => ops,
            Err(reason) => {
                log::warn!("cannot apply a workspace edit: {reason}");
                reply.send(false, Some(reason));
                return;
            }
        };
        if ops.is_empty() {
            reply.send(true, None);
            return;
        }
        let request = EditRequest {
            ops,
            encoding,
            label,
            reply,
        };
        cx.emit(LspBridgeEvent::ApplyEdit(EditHandle(Arc::new(request))));
    }
}
