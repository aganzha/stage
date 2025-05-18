// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later
use std::path::Path;

pub fn choose_parser(path: &Path) -> Option<tree_sitter::Parser> {
    let path_str = path.to_str().unwrap();
    if path_str.ends_with(".rs") {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Error loading Rust grammar");
        return Some(parser);
    }
    None
}

pub fn get_node_range<'a>(
    node: &tree_sitter::Node<'a>,
    cursor: &mut tree_sitter::TreeCursor<'a>,
    acc: &mut Vec<(i32, i32)>,
) {
    match node.kind() {
        "pub" | "fn" | "let" | "mut" | "if" | "else" | "loop" | "while" | "for" | "match"
        | "return" | "break" | "continue" | "struct" | "enum" | "impl" | "trait" | "use"
        | "const" | "static" => {
            acc.push((
                node.start_position().column as i32,
                node.end_position().column as i32,
            ));
        }
        _ => {}
    }

    // Move the cursor to the first child
    if cursor.goto_first_child() {
        loop {
            // Recursively call get_node_range for the current child
            get_node_range(&cursor.node(), cursor, acc);
            // Move to the next sibling
            if !cursor.goto_next_sibling() {
                break; // Exit the loop if there are no more siblings
            }
        }
        // Move the cursor back to the parent after processing all children
        cursor.goto_parent();
    }
}

pub fn collect_ranges(content: &str, parser: &mut tree_sitter::Parser) -> Vec<(i32, i32)> {
    let tree = parser.parse(content, None).unwrap();
    let root_node = tree.root_node();
    let mut cursor = root_node.walk();
    let mut result = Vec::new();
    get_node_range(&root_node, &mut cursor, &mut result);
    result
}
