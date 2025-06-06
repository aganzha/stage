// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Hunk, Line};
use log::trace;
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
                "self", "in",
            ],
            LanguageWrapper::Python(_) => vec![
                "self",
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
                "await",
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
    acc: &mut Vec<(usize, usize)>,
    acc_1: &mut Vec<(usize, usize)>,
    parent_kind: &'static str,
    language: &LanguageWrapper,
) {
    let keywords = language.keywords();

    if keywords.contains(&node.kind()) {
        trace!("keyword node {:?}", node.kind());
        acc.push((node.start_byte(), node.end_byte()));
    } else if node.kind() == "identifier" {
        if let Some(field_name) = cursor.field_name() {
            trace!(
                "identifier node {:?} {:?} {:?}",
                parent_kind,
                field_name,
                node
            );
            match (language, parent_kind, field_name) {
                (
                    LanguageWrapper::Rust(_),
                    "parameter" | "tuple_struct_pattern" | "let_declaration",
                    "pattern",
                ) => {
                    trace!("parent > field {:?} {:?}", parent_kind, field_name);
                    acc_1.push((node.start_byte(), node.end_byte()))
                }
                (LanguageWrapper::Rust(_), "field_expression", "value") => {
                    trace!("parent > field {:?} {:?}", parent_kind, field_name);
                    acc_1.push((node.start_byte(), node.end_byte()))
                }
                (LanguageWrapper::Python(_), "assignment", "left") => {
                    trace!("parent > field {:?} {:?}", parent_kind, field_name);
                    acc_1.push((node.start_byte(), node.end_byte()))
                }
                (LanguageWrapper::TypeScript(_), "variable_declarator", "name") => {
                    trace!("parent > field {:?} {:?}", parent_kind, field_name);
                    acc_1.push((node.start_byte(), node.end_byte()))
                }
                (_, _, _) => {}
            }
        } else {
            trace!("nooooooooo name {:?} {:?}", parent_kind, node);
        }
    }
    if cursor.goto_first_child() {
        loop {
            get_node_range(&cursor.node(), cursor, acc, acc_1, node.kind(), language);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

pub fn collect_ranges(
    content: &str,
    parser: &mut LanguageWrapper,
) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let tree = match parser {
        LanguageWrapper::Rust(p) => p.parse(content, None).unwrap(),
        LanguageWrapper::Python(p) => p.parse(content, None).unwrap(),
        LanguageWrapper::TypeScript(p) => p.parse(content, None).unwrap(),
    };

    let root_node = tree.root_node();
    let mut cursor = root_node.walk();
    let mut result = Vec::new();
    let mut result_1 = Vec::new();
    get_node_range(
        &root_node,
        &mut cursor,
        &mut result,
        &mut result_1,
        "",
        parser,
    );
    (result, result_1)
}

impl Line {
    pub fn byte_indexes_to_char_indexes(&self, byte_indexes: &[(usize, usize)]) -> Vec<(i32, i32)> {
        byte_indexes
            .iter()
            .filter(|(from, to)| {
                *from >= self.content_idx.0
                    && *to <= self.content_idx.0 + self.content_idx.1
                    && from != to
            })
            .filter_map(|(from, to)| {
                let byte_start = from - self.content_idx.0;
                let first_char_no = self.char_indices.get(&byte_start)?;
                // the byte offset right after the last character
                let byte_end = to - self.content_idx.0;
                let last_char_no = if let Some(last_char_no) = self.char_indices.get(&byte_end) {
                    last_char_no
                } else {
                    // in case of unicode letter there will be 2 bytes
                    // testё - char index is 5. byte index is 6
                    self.char_indices.get(&(byte_end - 1))?
                };
                Some((*first_char_no, *last_char_no))
            })
            .collect()
    }
    // hop
    pub fn fill_char_indices(&mut self, buf: &str) {
        for (i, (byte_index, _)) in buf[self.content_idx.0..self.content_idx.0 + self.content_idx.1]
            .char_indices()
            .enumerate()
        {
            self.char_indices.insert(byte_index, i as i32);
        }
    }
}

impl Hunk {
    pub fn parse_syntax(&mut self, parser: Option<&mut LanguageWrapper>) {
        if let Some(parser) = parser {
            (self.keyword_ranges, self.identifier_ranges) = collect_ranges(&self.buf, parser);
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::HunkLineNo;
    use crate::status_view::view::View;
    use crate::{Line, LineKind};
    use git2::DiffLineType;
    use std::collections::HashMap;
    #[test]
    fn test_byte_indexes_to_char_indexes_edge_cases() {
        for buf in vec!["abc🌄defhij", "abcdefhij"] {
            let mut line = Line {
                origin: DiffLineType::Context,
                view: View::new(),
                new_line_no: Some(HunkLineNo::new(0)),
                old_line_no: Some(HunkLineNo::new(0)),
                kind: LineKind::None,
                content_idx: (0, buf.len()),
                char_indices: HashMap::new(),
            };
            line.fill_char_indices(buf);
            let byte_indexes = vec![(0, buf.len())];
            let expected: Vec<(i32, i32)> = vec![(0, (buf.chars().count() - 1) as i32)];
            assert_eq!(line.byte_indexes_to_char_indexes(&byte_indexes), expected);
        }
    }
}
