//! Built-in snippet sets: `(languages, [(prefix, body, description)])`.

type Def = (&'static str, &'static str, &'static str);

const JS_LIKE: &[&str] = &[
    "javascript",
    "typescript",
    "javascriptreact",
    "typescriptreact",
];

const RUST: &[Def] = &[
    (
        "fn",
        "fn ${1:name}(${2}) {\n\t$0\n}",
        "function",
    ),
    ("main", "fn main() {\n\t$0\n}", "main function"),
    (
        "test",
        "#[test]\nfn ${1:name}() {\n\t$0\n}",
        "test function",
    ),
    ("if", "if ${1:condition} {\n\t$0\n}", "if block"),
    (
        "for",
        "for ${1:item} in ${2:iter} {\n\t$0\n}",
        "for loop",
    ),
    (
        "match",
        "match ${1:expr} {\n\t${2:pattern} => ${3:()},\n\t_ => $0,\n}",
        "match expression",
    ),
    (
        "struct",
        "struct ${1:Name} {\n\t${2:field}: ${3:Type},\n}",
        "struct",
    ),
    ("impl", "impl ${1:Type} {\n\t$0\n}", "impl block"),
    (
        "modtest",
        "#[cfg(test)]\nmod tests {\n\tuse super::*;\n\n\t#[test]\n\tfn ${1:name}() {\n\t\t$0\n\t}\n}",
        "test module",
    ),
];

const PYTHON: &[Def] = &[
    (
        "def",
        "def ${1:name}(${2}):\n\t${0:pass}",
        "function",
    ),
    (
        "class",
        "class ${1:Name}:\n\tdef __init__(self${2}):\n\t\t${0:pass}",
        "class",
    ),
    (
        "ifmain",
        "if __name__ == \"__main__\":\n\t${0:main()}",
        "if __name__ == \"__main__\"",
    ),
    ("if", "if ${1:condition}:\n\t${0:pass}", "if block"),
    (
        "for",
        "for ${1:item} in ${2:iterable}:\n\t${0:pass}",
        "for loop",
    ),
];

const JS: &[Def] = &[
    (
        "function",
        "function ${1:name}(${2}) {\n\t$0\n}",
        "function",
    ),
    ("arrow", "(${1}) => {\n\t$0\n}", "arrow function"),
    ("log", "console.log(${1});$0", "console.log"),
    ("if", "if (${1:condition}) {\n\t$0\n}", "if block"),
    (
        "for",
        "for (const ${1:item} of ${2:items}) {\n\t$0\n}",
        "for...of loop",
    ),
    (
        "class",
        "class ${1:Name} {\n\tconstructor(${2}) {\n\t\t$0\n\t}\n}",
        "class",
    ),
];

const GO: &[Def] = &[
    (
        "func",
        "func ${1:name}(${2}) {\n\t$0\n}",
        "function",
    ),
    ("main", "func main() {\n\t$0\n}", "main function"),
    (
        "iferr",
        "if err != nil {\n\treturn ${1:err}\n}",
        "if err != nil",
    ),
    ("if", "if ${1:condition} {\n\t$0\n}", "if block"),
    (
        "for",
        "for ${1:i} := ${2:0}; $1 < ${3:n}; $1++ {\n\t$0\n}",
        "for loop",
    ),
    (
        "struct",
        "type ${1:Name} struct {\n\t$0\n}",
        "struct type",
    ),
    (
        "test",
        "func Test${1:Name}(t *testing.T) {\n\t$0\n}",
        "test function",
    ),
];

/// All built-in sets.
pub const BUILTIN: &[(&[&str], &[Def])] = &[
    (&["rust"], RUST),
    (&["python"], PYTHON),
    (JS_LIKE, JS),
    (&["go"], GO),
];
