// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Parser;

pub fn choose_parser(path: &Path) -> Option<LanguageWrapper> {
    let path_str = path.to_str().unwrap();
    let mut parser = Parser::new();

    if path_str.ends_with(".rs") {
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Error loading Rust grammar");
        return Some(LanguageWrapper::Rust(parser));
    }
    if path_str.ends_with(".py") {
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("Error loading Python grammar");
        return Some(LanguageWrapper::Python(parser));
    }
    if path_str.ends_with(".ts") {
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("Error loading TypeScript grammar");
        return Some(LanguageWrapper::TypeScript(parser));
    }
    if path_str.ends_with(".tsx") {
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
            .expect("Error loading TSX grammar");
        return Some(LanguageWrapper::TypeScript(parser)); // Treat TSX as TypeScript
    }
    None
}

pub enum LanguageWrapper {
    Rust(Parser),
    Python(Parser),
    TypeScript(Parser),
}

impl LanguageWrapper {
    pub fn keywords(&self) -> Vec<&'static str> {
        match self {
            LanguageWrapper::Rust(_) => vec![
                "pub", "fn", "let", "mut", "if", "else", "loop", "while", "for", "match", "return",
                "break", "continue", "struct", "enum", "impl", "trait", "use", "const", "static",
                "self",
            ],
            LanguageWrapper::Python(_) => vec![
                "False",
                "None",
                "True",
                "and",
                "as",
                "assert",
                "async",
                "await",
                "break",
                "class",
                "continue",
                "def",
                "del",
                "elif",
                "else",
                "except",
                "finally",
                "for",
                "from",
                "global",
                "if",
                "import",
                "in",
                "is",
                "lambda",
                "nonlocal",
                "not",
                "or",
                "pass",
                "raise",
                "return",
                "try",
                "while",
                "with",
                "yield",
                "int",
                "float",
                "complex",
                "str",
                "dict",
                "set",
                "frozenset",
                "bool",
                "bytes",
                "bytearray",
                "memoryview",
            ],
            LanguageWrapper::TypeScript(_) => vec![
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "debugger",
                "default",
                "delete",
                "do",
                "else",
                "enum",
                "export",
                "extends",
                "false",
                "finally",
                "for",
                "function",
                "if",
                "implements",
                "import",
                "in",
                "instanceof",
                "interface",
                "let",
                "new",
                "null",
                "return",
                "super",
                "switch",
                "this",
                "throw",
                "try",
                "true",
                "type",
                "typeof",
                "var",
                "void",
                "while",
                "with",
                "yield",
                "any",
                "unknown",
                "void",
                "never",
                "boolean",
                "number",
                //"string", got broken on cyrylic strings
                "symbol",
                "bigint",
            ],
        }
    }
}

pub fn get_node_range<'a>(
    node: &tree_sitter::Node<'a>,
    cursor: &mut tree_sitter::TreeCursor<'a>,
    acc: &mut Vec<(i32, i32)>,
    acc_1: &mut Vec<(i32, i32)>,
    language: &LanguageWrapper,
) {
    let keywords = language.keywords();

    if keywords.contains(&node.kind()) {
        acc.push((
            node.start_position().column as i32,
            node.end_position().column as i32,
        ));
    } else if node.kind() == "identifier" {
        acc_1.push((
            node.start_position().column as i32,
            node.end_position().column as i32,
        ))
    }

    // Move the cursor to the first child
    if cursor.goto_first_child() {
        loop {
            // Recursively call get_node_range for the current child
            get_node_range(&cursor.node(), cursor, acc, acc_1, language);
            // Move to the next sibling
            if !cursor.goto_next_sibling() {
                break; // Exit the loop if there are no more siblings
            }
        }
        // Move the cursor back to the parent after processing all children
        cursor.goto_parent();
    }
}

pub fn collect_ranges(
    content: &str,
    parser: &mut LanguageWrapper,
) -> (Vec<(i32, i32)>, Vec<(i32, i32)>) {
    // bytes to chars for utf-8
    let mut mapping = HashMap::new();
    for (current_index, (byte_index, _)) in content.char_indices().enumerate() {
        mapping.insert(byte_index as i32, current_index as i32);
    }

    let tree = match parser {
        LanguageWrapper::Rust(p) => p.parse(content, None).unwrap(),
        LanguageWrapper::Python(p) => p.parse(content, None).unwrap(),
        LanguageWrapper::TypeScript(p) => p.parse(content, None).unwrap(),
    };

    let root_node = tree.root_node();
    let mut cursor = root_node.walk();
    let mut result = Vec::new();
    let mut result_1 = Vec::new();
    // Get the keywords for the current language
    let language = parser; // We already have the language in the parser
    get_node_range(
        &root_node,
        &mut cursor,
        &mut result,
        &mut result_1,
        language,
    );
    let mx = content.chars().count() as i32;
    let char_result = result
        .into_iter()
        .map(|(from, to)| {
            (
                *mapping.get(&from).unwrap_or(&0),
                *mapping.get(&to).unwrap_or(&mx),
            )
        })
        .collect::<Vec<(i32, i32)>>();
    let char_result_1 = result_1
        .into_iter()
        .map(|(from, to)| {
            (
                *mapping.get(&from).unwrap_or(&0),
                *mapping.get(&to).unwrap_or(&mx),
            )
        })
        .collect::<Vec<(i32, i32)>>();
    (char_result, char_result_1)
}
