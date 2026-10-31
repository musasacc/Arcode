//! The command line for a debuggee that an adapter asks to run in a
//! terminal (`runInTerminal`).

/// The shell line that runs a `runInTerminal` request's program with its
/// environment changes.
pub(crate) fn debuggee_command_line(args: &[String], env: &[(String, Option<String>)]) -> String {
    let Some((program, rest)) = args.split_first() else {
        return String::new();
    };
    let command = arcode_project::command_line(program, rest);
    if env.is_empty() {
        return command;
    }
    if cfg!(windows) {
        let mut line = String::new();
        for (k, v) in env {
            match v {
                Some(v) => line.push_str(&format!("$env:{k}={}; ", arcode_project::quote_arg(v))),
                None => line.push_str(&format!("Remove-Item Env:{k} -ErrorAction Ignore; ")),
            }
        }
        line + &command
    } else {
        let mut line = String::from("env");
        for (k, v) in env {
            match v {
                Some(v) => {
                    line.push(' ');
                    line.push_str(&arcode_project::quote_arg(&format!("{k}={v}")));
                }
                None => line.push_str(&format!(" -u {k}")),
            }
        }
        format!("{line} {command}")
    }
}

#[cfg(test)]
mod tests {
    use super::debuggee_command_line;

    #[test]
    fn debuggee_lines_quote_and_set_the_environment() {
        let args = vec!["/bin/app".to_string(), "a b".to_string()];
        assert_eq!(debuggee_command_line(&args, &[]), "/bin/app 'a b'");
        let env = vec![("K".to_string(), Some("v 1".to_string()))];
        let line = debuggee_command_line(&args, &env);
        if cfg!(windows) {
            assert!(line.starts_with("$env:K='v 1'; "), "{line}");
        } else {
            assert_eq!(line, "env 'K=v 1' /bin/app 'a b'");
        }
        assert_eq!(debuggee_command_line(&[], &env), "");
    }
}
