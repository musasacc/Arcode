//! Outline symbols: Rust, Python, JavaScript, TypeScript, Go.

mod common;

use arcode_syntax::OutlineKind::{
    Class, Const, Constructor, Enum, Function, Impl, Interface, Macro, Method, Module, Struct,
    Trait, Type,
};
use common::{o, outline, parse};

const RUST: &str = r#"const LIMIT: usize = 10;
static NAME: &str = "x";

pub struct Point {
    x: i32,
}

enum Shape { Circle, Square }

pub trait Draw {
    fn draw(&self);
    fn size(&self) -> usize { 0 }
}

impl Point {
    pub fn new() -> Self {
        Point { x: 0 }
    }
}

impl<T: Clone> Draw for Wrapper<T> {
    fn draw(&self) {}
}

type Alias = Vec<u8>;

macro_rules! square {
    ($x:expr) => { $x * $x };
}

mod tests {
    fn helper() {}
}

fn main() {
    fn inner() {}
}
"#;

#[test]
fn rust_outline() {
    assert_eq!(
        outline("rust", RUST),
        vec![
            o(0, Const, "LIMIT"),
            o(0, Const, "NAME"),
            o(0, Struct, "Point"),
            o(0, Enum, "Shape"),
            o(0, Trait, "Draw"),
            o(1, Method, "draw"),
            o(1, Method, "size"),
            o(0, Impl, "Point"),
            o(1, Method, "new"),
            o(0, Impl, "Draw for Wrapper<T>"),
            o(1, Method, "draw"),
            o(0, Type, "Alias"),
            o(0, Macro, "square"),
            o(0, Module, "tests"),
            o(1, Function, "helper"),
            o(0, Function, "main"),
            o(1, Function, "inner"),
        ]
    );
}

#[test]
fn rust_outline_ranges() {
    let (text, syntax) = parse("rust", RUST);
    let items = syntax.symbols(&text);
    let point = items.iter().find(|i| i.name == "Point").expect("Point");
    assert_eq!(point.range, 3..6);
    let impl_point = items
        .iter()
        .find(|i| i.kind == Impl && i.name == "Point")
        .expect("impl");
    assert_eq!(impl_point.range, 14..19);
    let new = items.iter().find(|i| i.name == "new").expect("new");
    assert_eq!(new.range, 15..18);
    assert_eq!(&RUST[new.name_range.clone()], "new");
    assert!(RUST[new.byte_range.clone()].starts_with("pub fn new"));
    let imp = items
        .iter()
        .find(|i| i.name == "Draw for Wrapper<T>")
        .expect("trait impl");
    assert_eq!(&RUST[imp.name_range.clone()], "Draw for Wrapper<T>");
}

#[test]
fn python_outline() {
    let src = r#"MAX_SIZE = 3
lower = 1

def top(a):
    def nested():
        pass
    return a

class Shape:
    def __init__(self):
        self.x = 1

    @property
    def area(self):
        return 0

    class Inner:
        pass
"#;
    assert_eq!(
        outline("python", src),
        vec![
            o(0, Const, "MAX_SIZE"),
            o(0, Function, "top"),
            o(1, Function, "nested"),
            o(0, Class, "Shape"),
            o(1, Method, "__init__"),
            o(1, Method, "area"),
            o(1, Class, "Inner"),
        ]
    );
}

const JS: &str = r#"export function load(path) {
  return path;
}

const helper = () => 1;
export const format = function (x) { return x; };

class Store extends Base {
  constructor() {
    super();
  }

  get size() { return 0; }

  async save() {
    const local = () => 2;
  }
}

function* ids() {}
"#;

#[test]
fn javascript_outline() {
    let expected = vec![
        o(0, Function, "load"),
        o(0, Function, "helper"),
        o(0, Function, "format"),
        o(0, Class, "Store"),
        o(1, Constructor, "constructor"),
        o(1, Method, "size"),
        o(1, Method, "save"),
        o(0, Function, "ids"),
    ];
    for id in [
        "javascript",
        "javascriptreact",
        "typescript",
        "typescriptreact",
    ] {
        assert_eq!(outline(id, JS), expected, "{id}");
    }
}

#[test]
fn typescript_outline() {
    let src = r#"export interface Shape {
  area(): number;
  name: string;
}

type Id = string | number;

enum Color { Red, Green }

namespace Geometry {
  export function scale(s: Shape, k: number): Shape { return s; }
}

abstract class Base<T> {
  abstract draw(): void;
  protected run(x: T): T { return x; }
}

declare function external(a: number): void;
"#;
    let expected = vec![
        o(0, Interface, "Shape"),
        o(1, Method, "area"),
        o(0, Type, "Id"),
        o(0, Enum, "Color"),
        o(0, Module, "Geometry"),
        o(1, Function, "scale"),
        o(0, Class, "Base"),
        o(1, Method, "draw"),
        o(1, Method, "run"),
        o(0, Function, "external"),
    ];
    assert_eq!(outline("typescript", src), expected);
    assert_eq!(outline("typescriptreact", src), expected);
}

#[test]
fn go_outline() {
    let src = r#"package shapes

const Pi = 3.14

type Point struct {
	X, Y int
}

type Shape interface {
	Area() float64
}

type ID = string

type Celsius float64

func New() *Point { return &Point{} }

func (p *Point) Move(dx int) {}

func (l List[T]) Len() int { return 0 }
"#;
    assert_eq!(
        outline("go", src),
        vec![
            o(0, Const, "Pi"),
            o(0, Struct, "Point"),
            o(0, Interface, "Shape"),
            o(1, Method, "Area"),
            o(0, Type, "ID"),
            o(0, Type, "Celsius"),
            o(0, Function, "New"),
            o(0, Method, "Point.Move"),
            o(0, Method, "List.Len"),
        ]
    );
}
