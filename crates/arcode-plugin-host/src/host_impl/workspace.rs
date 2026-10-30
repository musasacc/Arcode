//! `arcode:plugin/workspace`.

use globset::GlobBuilder;

use crate::bindings::arcode::plugin::workspace::{self as wit, FsError};
use crate::consent::FileGrant;
use crate::state::HostState;

/// `find-files` never returns more than this many paths.
pub const MAX_FIND_FILES: u32 = 10_000;

impl wit::Host for HostState {
    fn root_name(&mut self) -> wasmtime::Result<Option<String>> {
        // Only the display name; the absolute path never reaches a plugin.
        Ok(self.ctx.project.as_ref().map(|p| p.name.clone()))
    }

    fn find_files(
        &mut self,
        glob: String,
        limit: u32,
    ) -> wasmtime::Result<Result<Vec<String>, FsError>> {
        if self.ctx.project.is_none() || self.ctx.files == FileGrant::None {
            return Ok(Err(FsError::Denied));
        }
        let matcher = match GlobBuilder::new(&glob)
            .literal_separator(true)
            .build()
            .map(|g| g.compile_matcher())
        {
            Ok(m) => m,
            Err(e) => return Ok(Err(FsError::InvalidGlob(e.kind().to_string()))),
        };
        let limit = limit.min(MAX_FIND_FILES) as usize;
        let mut files = self.ctx.services.index.find_files(&matcher, limit);
        files.truncate(limit);
        Ok(Ok(files))
    }
}
