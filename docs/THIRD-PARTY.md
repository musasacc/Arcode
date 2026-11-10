# Third-party software in Arcode releases

Arcode itself is MIT OR Apache-2.0. Release packages also contain
language servers by other authors (docs/PACKAGING.md, "Bundled language
servers"). They are separate programs that Arcode starts as child
processes and talks to over the Language Server Protocol; none of them is
linked into Arcode, and each is distributed unmodified under its own
license.

Every package carries, for its platform:

- `share/doc/arcode/THIRD-PARTY.txt`: each bundled component with its
  version, license and homepage (where its source code is), the npm
  packages installed from the lockfile with their licenses, and what was
  left out;
- `share/arcode/language-servers/LICENSES/`: the license texts, one
  folder per component (`LICENSES/npm/<package>/` for every npm package).

| Component | License | Notes |
|---|---|---|
| Node.js | MIT | Its LICENSE also holds the notices of the libraries it includes (V8, OpenSSL, ICU, libuv, ...). Only the `node` executable is shipped. |
| rust-analyzer | MIT OR Apache-2.0 | |
| clangd | Apache-2.0 WITH LLVM-exception | |
| gopls | BSD-3-Clause | Built from the pinned module, checked against the Go checksum database. Its telemetry counters stay on the machine unless the user runs `gopls telemetry on`. |
| pyright | MIT | |
| ruff | MIT | |
| typescript-language-server | Apache-2.0 | |
| TypeScript | Apache-2.0 | |
| vscode-langservers-extracted | MIT | Servers extracted from VS Code (MIT). |
| yaml-language-server | MIT | |
| bash-language-server | MIT | |
| lua-language-server | MIT | |
| marksman | MIT | Self-contained .NET build (MIT). |
| taplo | MIT | |
| Eclipse JDT Language Server (jdtls) | EPL-2.0 | Unmodified binary distribution; the source is at https://github.com/eclipse-jdtls/eclipse.jdt.ls. Runs on the user's own Java. |

Not bundled, on purpose:

- **intelephense** (PHP): its licence grants a personal, non-transferable
  licence and forbids redistribution, so it cannot be shipped. The
  Languages page keeps its install command (`npm install --global
  intelephense`).
- **ruby-lsp** (Ruby): MIT, but it runs inside the project's Ruby and
  Bundler and has to be installed with them (`gem install ruby-lsp`).

Before adding a server to `packaging/language-servers.toml`, check that
its license allows redistribution, that it does not send telemetry by
default (or that a built-in setting turns it off), and list its license
files in the manifest.

## Language logos (file icons)

The file icons in `assets/file-icons/` are compiled into the Arcode
binary. They are 48 SVG logos from Devicon 2.17.0 (npm package
`devicon`, https://github.com/devicons/devicon), MIT licence, Copyright
(c) 2015 konpa; the licence text is in `assets/file-icons/LICENSE`.

- Mostly the full-colour "original" variants; the "plain" variant where
  the original does not read at 16 px or is a mascot (Bash, Docker,
  GraphQL, JSON, Lua, Perl, PowerShell, Vim, YAML), and the
  "original-wordmark" for Go.
- Stripped of metadata (comments, editor attributes, unused style
  properties) and otherwise unchanged.
- The logos are trademarks of their respective owners. They are used
  only to show which language a file is written in, and their use does
  not imply endorsement by the owners.

`crates/arcode-ui-kit/src/file_logo/mod.rs` lists them, and
`ui.file_icons = "plain"` turns them off.
