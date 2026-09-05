use super::*;
use crate::tools::outline::grammar::SupportedLanguage;
use crate::tools::outline::types::SymbolKind;

#[test]
fn test_parse_rust_symbols() {
    let source = r#"
pub struct AgentEngineBuilder {
    pub config: Config,
}

pub enum SymbolKind {
    Function,
    Method,
}

pub trait Runner {
    fn run(&self) -> bool;
}

pub type AppResult<T> = Result<T, String>;

impl AgentEngineBuilder {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn build(self) -> Result<AgentEngine, String> {
        Ok(AgentEngine)
    }
}

pub fn standalone_helper() -> i32 {
    42
}
"#;

    let entries = parse_symbols(source, SupportedLanguage::Rust).expect("parse rust symbols");

    assert!(entries.iter().any(|e| e.name == "AgentEngineBuilder"
        && e.kind == SymbolKind::Struct
        && e.signature.starts_with("pub struct AgentEngineBuilder")
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "SymbolKind"
        && e.kind == SymbolKind::Enum
        && e.signature == "pub enum SymbolKind"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "Runner"
        && e.kind == SymbolKind::Trait
        && e.signature == "pub trait Runner"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "AppResult"
        && e.kind == SymbolKind::Type
        && e.signature.starts_with("pub type AppResult<T>")
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "AgentEngineBuilder"
        && e.kind == SymbolKind::Impl
        && e.signature == "impl AgentEngineBuilder"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "new"
        && e.kind == SymbolKind::Method
        && e.signature == "pub fn new(config: Config) -> Self"
        && e.depth == 1));

    assert!(entries.iter().any(|e| e.name == "build"
        && e.kind == SymbolKind::Method
        && e.signature == "pub async fn build(self) -> Result<AgentEngine, String>"
        && e.depth == 1));

    assert!(entries.iter().any(|e| e.name == "standalone_helper"
        && e.kind == SymbolKind::Function
        && e.signature == "pub fn standalone_helper() -> i32"
        && e.depth == 0));
}

#[test]
fn test_parse_typescript_symbols() {
    let source = r#"
export class UserService implements IUserService {
    constructor(config: Config) {
        this.config = config;
    }

    public async getUser(id: string): Promise<User> {
        return fetchUser(id);
    }
}

export interface User {
    id: string;
    name: string;
}

export type UserId = string | number;

export enum Role {
    Admin,
    Member,
}

export function createUserService(): UserService {
    return new UserService();
}
"#;

    let entries = parse_symbols(source, SupportedLanguage::TypeScript).expect("parse ts symbols");

    assert!(entries.iter().any(|e| e.name == "UserService"
        && e.kind == SymbolKind::Class
        && e.signature.starts_with("export class UserService")
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "constructor"
        && e.kind == SymbolKind::Method
        && e.signature == "constructor(config: Config)"
        && e.depth == 1));

    assert!(entries.iter().any(|e| e.name == "getUser"
        && e.kind == SymbolKind::Method
        && e.signature == "public async getUser(id: string): Promise<User>"
        && e.depth == 1));

    assert!(entries.iter().any(|e| e.name == "User"
        && e.kind == SymbolKind::Interface
        && e.signature == "export interface User"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "UserId"
        && e.kind == SymbolKind::Type
        && e.signature.starts_with("export type UserId = string")
        && e.depth == 0));

    assert!(
        entries.iter().any(|e| e.name == "Role"
            && e.kind == SymbolKind::Enum
            && e.signature == "export enum Role"
            && e.depth == 0)
    );

    assert!(entries.iter().any(|e| e.name == "createUserService"
        && e.kind == SymbolKind::Function
        && e.signature == "export function createUserService(): UserService"
        && e.depth == 0));
}

#[test]
fn test_parse_python_symbols() {
    let source = r#"
def top_level(x: int) -> int:
    return x * 2

class Service:
    def __init__(self, name: str):
        self.name = name

    def execute(self) -> bool:
        def inner_helper():
            return True
        return inner_helper()
"#;

    let entries = parse_symbols(source, SupportedLanguage::Python).expect("parse python symbols");

    assert!(entries.iter().any(|e| e.name == "top_level"
        && e.kind == SymbolKind::Function
        && e.signature == "def top_level(x: int) -> int:"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "Service"
        && e.kind == SymbolKind::Class
        && e.signature == "class Service:"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "__init__"
        && e.kind == SymbolKind::Method
        && e.signature == "def __init__(self, name: str):"
        && e.depth == 1));

    assert!(entries.iter().any(|e| e.name == "execute"
        && e.kind == SymbolKind::Method
        && e.signature == "def execute(self) -> bool:"
        && e.depth == 1));

    assert!(entries.iter().any(|e| e.name == "inner_helper"
        && e.kind == SymbolKind::Function
        && e.signature == "def inner_helper():"
        && e.depth == 2));
}

#[test]
fn test_parse_go_symbols() {
    let source = r#"
package main

type Config struct {
    Port int
}

type Handler interface {
    Handle()
}

type ID = string

func Add(a, b int) int {
    return a + b
}

func (c *Config) Validate() error {
    return nil
}
"#;

    let entries = parse_symbols(source, SupportedLanguage::Go).expect("parse go symbols");

    assert!(entries.iter().any(|e| e.name == "Config"
        && e.kind == SymbolKind::Struct
        && e.signature == "type Config struct"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "Handler"
        && e.kind == SymbolKind::Interface
        && e.signature == "type Handler interface"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "ID"
        && e.kind == SymbolKind::Type
        && e.signature.starts_with("type ID = string")
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "Add"
        && e.kind == SymbolKind::Function
        && e.signature == "func Add(a, b int) int"
        && e.depth == 0));

    assert!(entries.iter().any(|e| e.name == "Validate"
        && e.kind == SymbolKind::Method
        && e.signature == "func (c *Config) Validate() error"
        && e.depth == 0));
}

#[test]
fn test_rust_outer_attributes_skipped() {
    let source = r#"
#[derive(Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    pub name: String,
}
"#;

    let entries = parse_symbols(source, SupportedLanguage::Rust).expect("parse rust");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Config");
    assert_eq!(entries[0].signature, "pub struct Config");
    assert_eq!(entries[0].line, 4);
}

#[test]
fn test_parse_javascript_symbols() {
    let source = r#"
class Calculator {
    constructor() {}
    add(a, b) { return a + b; }
}

function helper() {}
"#;

    let entries = parse_symbols(source, SupportedLanguage::JavaScript).expect("parse js");
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].name, "Calculator");
    assert_eq!(entries[0].kind, SymbolKind::Class);
    assert_eq!(entries[1].name, "constructor");
    assert_eq!(entries[1].kind, SymbolKind::Method);
    assert_eq!(entries[1].depth, 1);
    assert_eq!(entries[2].name, "add");
    assert_eq!(entries[2].kind, SymbolKind::Method);
    assert_eq!(entries[2].depth, 1);
    assert_eq!(entries[3].name, "helper");
    assert_eq!(entries[3].kind, SymbolKind::Function);
    assert_eq!(entries[3].depth, 0);
}

#[test]
fn test_symbol_kind_filter_matching() {
    assert!(SymbolKind::Function.matches("function"));
    assert!(SymbolKind::Function.matches("FUNCTION"));
    assert!(SymbolKind::Method.matches("method"));
    assert!(SymbolKind::Struct.matches("struct"));
    assert!(SymbolKind::Class.matches("class"));
    assert!(SymbolKind::Interface.matches("interface"));
    assert!(SymbolKind::Trait.matches("trait"));
    assert!(SymbolKind::Enum.matches("enum"));
    assert!(SymbolKind::Type.matches("type"));
    assert!(SymbolKind::Impl.matches("impl"));
    assert!(SymbolKind::Function.matches(""));
    assert!(!SymbolKind::Function.matches("struct"));
}

#[test]
fn test_parse_java_symbols() {
    let source = r#"
public class UserService implements IUserService {
    public UserService() {}

    public User getUser(String id) {
        return null;
    }
}

public interface IUserService {}
public record UserRecord(String id) {}
public enum Status { ACTIVE }
"#;

    let entries = parse_symbols(source, SupportedLanguage::Java).expect("parse java");
    assert!(entries.iter().any(|e| e.name == "UserService"
        && e.kind == SymbolKind::Class
        && e.signature == "public class UserService implements IUserService"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "UserService"
        && e.kind == SymbolKind::Method
        && e.signature == "public UserService()"
        && e.depth == 1));
    assert!(entries.iter().any(|e| e.name == "getUser"
        && e.kind == SymbolKind::Method
        && e.signature == "public User getUser(String id)"
        && e.depth == 1));
    assert!(entries.iter().any(|e| e.name == "IUserService"
        && e.kind == SymbolKind::Interface
        && e.signature == "public interface IUserService"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "UserRecord"
        && e.kind == SymbolKind::Class
        && e.signature == "public record UserRecord(String id)"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "Status"
        && e.kind == SymbolKind::Enum
        && e.signature == "public enum Status"
        && e.depth == 0));
}

#[test]
fn test_parse_c_symbols() {
    let source = r#"
struct Point {
    int x;
    int y;
};

typedef int UserID;

int add(int a, int b) {
    return a + b;
}
"#;

    let entries = parse_symbols(source, SupportedLanguage::C).expect("parse c");
    assert!(
        entries.iter().any(|e| e.name == "Point"
            && e.kind == SymbolKind::Struct
            && e.signature == "struct Point"
            && e.depth == 0)
    );
    assert!(entries.iter().any(|e| e.name == "UserID"
        && e.kind == SymbolKind::Type
        && e.signature == "typedef int UserID"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "add"
        && e.kind == SymbolKind::Function
        && e.signature == "int add(int a, int b)"
        && e.depth == 0));
}

#[test]
fn test_parse_cpp_symbols() {
    let source = r#"
namespace engine {
    class Renderer {
    public:
        void render() {}
    };

    int calculate(int x) {
        return x * 2;
    }
}
"#;

    let entries = parse_symbols(source, SupportedLanguage::Cpp).expect("parse cpp");
    assert!(entries.iter().any(|e| e.name == "Renderer"
        && e.kind == SymbolKind::Class
        && e.signature == "class Renderer"
        && e.depth == 1));
    assert!(
        entries
            .iter()
            .any(|e| e.name == "render" && e.kind == SymbolKind::Function
                || e.kind == SymbolKind::Method && e.signature == "void render()" && e.depth == 2)
    );
    assert!(entries.iter().any(|e| e.name == "calculate"
        && e.kind == SymbolKind::Function
        && e.signature == "int calculate(int x)"
        && e.depth == 1));
}

#[test]
fn test_parse_csharp_symbols() {
    let source = r#"
public class Service : IService {
    public Service() {}

    public void Execute() {}
}

public interface IService {}
public struct Point { public int X; }
public enum Priority { Low, High }
"#;

    let entries = parse_symbols(source, SupportedLanguage::CSharp).expect("parse csharp");
    assert!(entries.iter().any(|e| e.name == "Service"
        && e.kind == SymbolKind::Class
        && e.signature == "public class Service : IService"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "Execute"
        && e.kind == SymbolKind::Method
        && e.signature == "public void Execute()"
        && e.depth == 1));
    assert!(entries.iter().any(|e| e.name == "IService"
        && e.kind == SymbolKind::Interface
        && e.signature == "public interface IService"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "Point"
        && e.kind == SymbolKind::Struct
        && e.signature == "public struct Point"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "Priority"
        && e.kind == SymbolKind::Enum
        && e.signature == "public enum Priority"
        && e.depth == 0));
}

#[test]
fn test_parse_ruby_symbols() {
    let source = r#"
module Utils
  class Greeter
    def hello(name)
      puts "hello #{name}"
    end
  end
end
"#;

    let entries = parse_symbols(source, SupportedLanguage::Ruby).expect("parse ruby");
    assert!(
        entries
            .iter()
            .any(|e| e.name == "Utils" && e.signature == "module Utils" && e.depth == 0)
    );
    assert!(
        entries.iter().any(|e| e.name == "Greeter"
            && e.kind == SymbolKind::Class
            && e.signature == "class Greeter"
            && e.depth == 1)
    );
    assert!(entries.iter().any(|e| e.name == "hello"
        && e.kind == SymbolKind::Method
        && e.signature == "def hello(name)"
        && e.depth == 2));
}

#[test]
fn test_parse_php_symbols() {
    let source = r#"
<?php
interface Logger {
    public function log(string $msg);
}

class AppService implements Logger {
    public function log(string $msg) {}
}

function global_helper(): void {}
"#;

    let entries = parse_symbols(source, SupportedLanguage::Php).expect("parse php");
    assert!(entries.iter().any(|e| e.name == "Logger"
        && e.kind == SymbolKind::Interface
        && e.signature == "interface Logger"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "AppService"
        && e.kind == SymbolKind::Class
        && e.signature == "class AppService implements Logger"
        && e.depth == 0));
    assert!(entries.iter().any(|e| e.name == "log"
        && e.kind == SymbolKind::Method
        && e.signature == "public function log(string $msg)"
        && e.depth == 1));
    assert!(entries.iter().any(|e| e.name == "global_helper"
        && e.kind == SymbolKind::Function
        && e.signature == "function global_helper(): void"
        && e.depth == 0));
}

#[test]
fn test_empty_source_returns_empty_vec() {
    let entries = parse_symbols("", SupportedLanguage::Rust).expect("parse empty");
    assert!(entries.is_empty());
}
