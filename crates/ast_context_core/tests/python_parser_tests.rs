use std::path::Path;

use ast_context_core::parser::python::PythonParser;
use ast_context_core::parser::LanguageParser;

fn parse_source(source: &str) -> ast_context_core::types::FileParseResult {
    let parser = PythonParser::new();
    let path = Path::new("test.py");
    parser.parse(path, source.as_bytes(), false).unwrap()
}

#[test]
fn test_simple_function() {
    let result = parse_source("def hello():\n    print('world')");
    assert_eq!(result.functions.len(), 1);
    assert_eq!(result.functions[0].name, "hello");
    assert_eq!(result.functions[0].span.start_line, 1);
    assert!(result.functions[0].args.is_empty());
}

#[test]
fn test_function_with_args() {
    let result = parse_source("def add(a, b):\n    return a + b");
    assert_eq!(result.functions.len(), 1);
    assert_eq!(result.functions[0].name, "add");
    assert_eq!(result.functions[0].args, vec!["a", "b"]);
}

#[test]
fn test_function_with_typed_args() {
    let result = parse_source("def greet(name: str, count: int = 1) -> str:\n    return name * count");
    assert_eq!(result.functions.len(), 1);
    assert_eq!(result.functions[0].args, vec!["name", "count"]);
}

#[test]
fn test_function_with_splats() {
    let result = parse_source("def variadic(*args, **kwargs):\n    pass");
    assert_eq!(result.functions[0].args, vec!["*args", "**kwargs"]);
}

#[test]
fn test_class_basic() {
    let source = "class Greeter:\n    def greet(self):\n        pass";
    let result = parse_source(source);
    assert_eq!(result.classes.len(), 1);
    assert_eq!(result.classes[0].name, "Greeter");
    assert!(result.classes[0].bases.is_empty());
}

#[test]
fn test_class_with_inheritance() {
    let source = "class Animal:\n    pass\n\nclass Dog(Animal):\n    pass";
    let result = parse_source(source);
    assert_eq!(result.classes.len(), 2);
    assert_eq!(result.classes[0].name, "Animal");
    assert_eq!(result.classes[1].name, "Dog");
    assert_eq!(result.classes[1].bases, vec!["Animal"]);
}

#[test]
fn test_class_method_context() {
    let source = "class Foo:\n    def bar(self):\n        pass";
    let result = parse_source(source);
    assert_eq!(result.functions.len(), 1);
    assert_eq!(result.functions[0].name, "bar");
    assert_eq!(result.functions[0].class_context.as_deref(), Some("Foo"));
    assert_eq!(
        result.functions[0].context_type.as_deref(),
        Some("class_definition")
    );
}

#[test]
fn test_nested_function() {
    let source = "def outer():\n    def inner():\n        pass\n    inner()";
    let result = parse_source(source);
    assert_eq!(result.functions.len(), 2);
    let inner = result.functions.iter().find(|f| f.name == "inner").unwrap();
    assert_eq!(inner.context.as_deref(), Some("outer"));
    assert_eq!(
        inner.context_type.as_deref(),
        Some("function_definition")
    );
}

#[test]
fn test_import_simple() {
    let result = parse_source("import os");
    assert_eq!(result.imports.len(), 1);
    assert_eq!(result.imports[0].name, "os");
    assert_eq!(result.imports[0].full_import_name.as_deref(), Some("os"));
}

#[test]
fn test_import_from() {
    let result = parse_source("from pathlib import Path");
    assert_eq!(result.imports.len(), 1);
    assert_eq!(result.imports[0].name, "Path");
    assert_eq!(
        result.imports[0].full_import_name.as_deref(),
        Some("pathlib.Path")
    );
}

#[test]
fn test_import_aliased() {
    let result = parse_source("from typing import Optional as Opt");
    assert_eq!(result.imports.len(), 1);
    assert_eq!(result.imports[0].name, "Optional");
    assert_eq!(result.imports[0].alias.as_deref(), Some("Opt"));
}

#[test]
fn test_import_multiple_from() {
    let result = parse_source("from typing import List, Dict, Optional");
    assert_eq!(result.imports.len(), 3);
    let names: Vec<&str> = result.imports.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"List"));
    assert!(names.contains(&"Dict"));
    assert!(names.contains(&"Optional"));
}

#[test]
fn test_function_calls() {
    let source = "def foo():\n    print('hello')\n    bar(1, 2)";
    let result = parse_source(source);
    assert!(result.function_calls.len() >= 2);
    let names: Vec<&str> = result.function_calls.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"print"));
    assert!(names.contains(&"bar"));
}

#[test]
fn test_method_calls() {
    let source = "class Foo:\n    def bar(self):\n        self.baz()";
    let result = parse_source(source);
    let baz_call = result.function_calls.iter().find(|c| c.name == "baz").unwrap();
    assert_eq!(baz_call.full_name, "self.baz");
}

#[test]
fn test_variables() {
    let source = "x = 42\nname = 'hello'";
    let result = parse_source(source);
    assert_eq!(result.variables.len(), 2);
    let names: Vec<&str> = result.variables.iter().map(|v| v.name.as_str()).collect();
    assert!(names.contains(&"x"));
    assert!(names.contains(&"name"));
}

#[test]
fn test_lambda_assignment() {
    let source = "double = lambda x: x * 2";
    let result = parse_source(source);
    // Lambda assignments are captured as functions
    assert_eq!(result.functions.len(), 1);
    assert_eq!(result.functions[0].name, "double");
    assert_eq!(result.functions[0].args, vec!["x"]);
    assert_eq!(result.functions[0].cyclomatic_complexity, 1);
}

#[test]
fn test_cyclomatic_complexity() {
    let source = r#"
def complex(a, b):
    if a > b:
        for i in range(10):
            if i % 2 == 0:
                pass
    while a > 0:
        a -= 1
    return a
"#;
    let result = parse_source(source);
    assert_eq!(result.functions.len(), 1);
    // 1 base + if + for + if + while = 5
    assert_eq!(result.functions[0].cyclomatic_complexity, 5);
}

#[test]
fn test_docstring_extraction() {
    let source = "def documented():\n    \"\"\"This is a docstring.\"\"\"\n    pass";
    let result = parse_source(source);
    assert_eq!(result.functions.len(), 1);
    assert_eq!(
        result.functions[0].docstring.as_deref(),
        Some("This is a docstring.")
    );
}

#[test]
fn test_class_docstring() {
    let source = "class MyClass:\n    \"\"\"Class docstring.\"\"\"\n    pass";
    let result = parse_source(source);
    assert_eq!(result.classes.len(), 1);
    assert_eq!(
        result.classes[0].docstring.as_deref(),
        Some("Class docstring.")
    );
}

#[test]
fn test_decorators() {
    let source = "@staticmethod\ndef my_func():\n    pass";
    let result = parse_source(source);
    assert_eq!(result.functions.len(), 1);
    assert!(!result.functions[0].decorators.is_empty());
    assert!(result.functions[0].decorators[0].contains("staticmethod"));
}

#[test]
fn test_dedup_imports() {
    let source = "import os\nimport os";
    let result = parse_source(source);
    assert_eq!(result.imports.len(), 1);
}

#[test]
fn test_variable_not_lambda() {
    // Lambda assignments should not appear in variables
    let source = "x = 42\ndouble = lambda x: x * 2";
    let result = parse_source(source);
    let var_names: Vec<&str> = result.variables.iter().map(|v| v.name.as_str()).collect();
    assert!(var_names.contains(&"x"));
    assert!(!var_names.contains(&"double"));
}

#[test]
fn test_call_context() {
    let source = "def outer():\n    inner()";
    let result = parse_source(source);
    let call = result.function_calls.iter().find(|c| c.name == "inner").unwrap();
    assert!(call.context.is_some());
    let (ctx_name, ctx_type, _) = call.context.as_ref().unwrap();
    assert_eq!(ctx_name, "outer");
    assert_eq!(ctx_type, "function_definition");
}
