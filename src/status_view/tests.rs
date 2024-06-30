// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::status_view::render::{RenderFlags, View};
use crate::status_view::tags;

use crate::status_view::{StatusRenderContext, ViewContainer};
use crate::{Diff, DiffKind, File, Hunk, Line, LineKind};
use git2::DiffLineType;
use gtk4::prelude::*;
use gtk4::TextBuffer;
use log::debug;
use std::cell::Cell;
use std::sync::Once;
use regex::Regex;

static INIT: Once = Once::new();

pub fn initialize() {
    INIT.call_once(|| {
        env_logger::builder().format_timestamp(None).init();
        debug!("----------------> {:?}", gtk4::init());
    });
}

impl Hunk {
    // used in tests only
    pub fn fill_from_header(&mut self) {
        let re = Regex::new(r"@@ [+-]([0-9]+),([0-9]+) [+-]([0-9]+),([0-9]+) @@")
            .unwrap();
        if let Some((_, [old_start_s, old_lines_s, new_start_s, new_lines_s])) =
            re.captures_iter(&self.header).map(|c| c.extract()).next()
        {
            self.old_start = old_start_s.parse().expect("cant parse nums");
            self.old_lines = old_lines_s.parse().expect("cant parse nums");
            self.new_start = new_start_s.parse().expect("cant parse nums");
            self.new_lines = new_lines_s.parse().expect("cant parse nums");
            for line in &mut self.lines {
                if let Some(line_no) = line.new_line_no {
                    debug!("INCREMENT LINE_NO IN FILL BY HEADER me {:?} line {:?}", self.new_start, line_no);
                    line.new_line_no.replace(self.new_start + line_no);
                }
                if let Some(line_no) = line.old_line_no {
                    line.old_line_no.replace(self.old_start + line_no);
                }
            }
        };
    }
}

fn create_line(line_no: u32, name: &str) -> Line {
    Line {
        content: name.to_string(),
        origin: DiffLineType::Context,
        view: View::new(),
        new_line_no: Some(line_no),
        old_line_no: Some(line_no),
        kind: LineKind::None,
    }
}

fn create_hunk(name: &str) -> Hunk {
    let mut hunk = Hunk::new(DiffKind::Unstaged);
    hunk.handle_max(name);
    hunk.header = name.to_string();
    for i in 0..3 {
        let content = format!("{} -> line {}", hunk.header, i);
        hunk.handle_max(&content);      
        hunk.lines.push(create_line(i, &content));
    }
    hunk
}

fn create_file(name: &str) -> File {
    let mut file = File::new(DiffKind::Unstaged);
    file.path = name.to_string().into();
    for i in 0..3 {
        file.hunks
            .push(create_hunk(&format!("{} -> hunk {}", name, i)));
    }
    file
}

fn create_diff() -> Diff {
    let mut diff = Diff::new(DiffKind::Unstaged);
    for i in 0..3 {
        diff.files.push(create_file(&format!("file{}.rs", i)));
    }
    diff
}

pub fn mock_render_view(vc: &dyn ViewContainer, mut line_no: i32) -> i32 {
    let view = vc.get_view();
    view.line_no.replace(line_no);
    view.render(true);
    view.dirty(false);
    line_no += 1;
    if view.is_expanded() || view.is_child_dirty() {
        for child in vc.get_children() {
            line_no = mock_render_view(child, line_no)
        }
        vc.get_view().child_dirty(false);
    }
    line_no
}

pub fn mock_render(diff: &mut Diff) -> i32 {
    let mut line_no: i32 = 0;
    for file in &mut diff.files {
        line_no = mock_render_view(file, line_no);
    }
    line_no
}
// tests
pub fn cursor(diff: &mut Diff, line_no: i32, ctx: &mut StatusRenderContext) {
    for (_, file) in diff.files.iter_mut().enumerate() {
        file.cursor(line_no, false, ctx);
    }
    // some views will be rerenderred cause highlight changes
    mock_render(diff);
}

#[test]
pub fn test_file_active() {
    let mut diff = create_diff();
    mock_render(&mut diff);
    let mut context = StatusRenderContext::new();
    let mut line_no = (&diff.files[0]).view.line_no.get();
    cursor(&mut diff, line_no, &mut context);
    assert!((&diff.files[0]).view.is_current());
    assert!((&diff.files[0]).view.is_active());

    (&diff.files[0]).expand(line_no, &mut context);
    mock_render(&mut diff);

    // cursor is on file and file is expanded
    assert!((&diff.files[0]).view.is_current());
    assert!((&diff.files[0]).view.is_active());
    // file itself is active and everything inside file
    // is active
    for hunk in &diff.files[0].hunks {
        assert!(hunk.view.is_active());
        for line in &hunk.lines {
            assert!(line.view.is_active());
        }
    }
    line_no += 1;
    cursor(&mut diff, line_no, &mut context);
    assert!(!(&diff.files[0]).view.is_active());
    assert!(diff.files[0].hunks[0].view.is_rendered());
    assert!(diff.files[0].hunks[0].view.is_current());
    assert!(diff.files[0].hunks[0].view.is_active());
    for line in &diff.files[0].hunks[0].lines {
        assert!(line.view.is_rendered());
        assert!(line.view.is_active());
    }
}

#[test]
pub fn test_expand() {
    let mut diff = create_diff();
    mock_render(&mut diff);

    let mut context = StatusRenderContext::new();

    for cursor_line in 0..3 {
        cursor(&mut diff, cursor_line, &mut context);

        for (i, file) in diff.files.iter_mut().enumerate() {
            let view = file.get_view();
            if i as i32 == cursor_line {
                assert!(view.is_active());
                assert!(view.is_current());
            } else {
                assert!(!view.is_active());
                assert!(!view.is_current());
            }
            assert!(!view.is_expanded());
        }
    }
    // last line from prev loop
    // the cursor is on it
    let mut cursor_line = 2;
    for file in &mut diff.files {
        if let Some(_expanded_line) = file.expand(cursor_line, &mut context) {
            assert!(file.get_view().is_child_dirty());
            break;
        }
    }

    mock_render(&mut diff);

    for (i, file) in diff.files.iter_mut().enumerate() {
        let view = file.get_view();
        if i as i32 == cursor_line {
            assert!(view.is_rendered());
            assert!(view.is_current());
            assert!(view.is_active());
            assert!(view.is_expanded());
            file.walk_down(&mut |vc: &dyn ViewContainer| {
                let view = vc.get_view();
                assert!(view.is_rendered());
                assert!(view.is_active());
                assert!(!view.is_squashed());
                assert!(!view.is_current());
            });
        } else {
            assert!(!view.is_current());
            assert!(!view.is_active());
            assert!(!view.is_expanded());
            file.walk_down(&|vc: &dyn ViewContainer| {
                let view = vc.get_view();
                assert!(!view.is_rendered());
            });
        }
    }

    // go 1 line backward
    // end expand it
    cursor_line = 1;
    cursor(&mut diff, cursor_line, &mut context);

    for file in &mut diff.files {
        if let Some(_expanded_line) = file.expand(cursor_line, &mut context) {
            break;
        }
    }

    mock_render(&mut diff);
    for (i, file) in diff.files.iter_mut().enumerate() {
        let view = file.get_view();
        let j = i as i32;
        if j < cursor_line {
            // all are inactive
            assert!(!view.is_current());
            assert!(!view.is_active());
            assert!(!view.is_expanded());
            file.walk_down(&|vc: &dyn ViewContainer| {
                let view = vc.get_view();
                assert!(!view.is_rendered());
            });
        } else if j == cursor_line {
            // all are active
            assert!(view.is_rendered());
            assert!(view.is_current());
            assert!(view.is_active());
            assert!(view.is_expanded());
            file.walk_down(&|vc: &dyn ViewContainer| {
                let view = vc.get_view();
                assert!(view.is_rendered());
                assert!(view.is_active());
                assert!(!view.is_current());
            });
        } else if j > cursor_line {
            // all are expanded but inactive
            assert!(view.is_rendered());
            assert!(!view.is_current());
            assert!(!view.is_active());
            assert!(view.is_expanded());
            file.walk_down(&|vc: &dyn ViewContainer| {
                let view = vc.get_view();
                assert!(view.is_rendered());
                assert!(!view.is_active());
                assert!(!view.is_current());
            });
        }
    }

    // go to first hunk of second file
    cursor_line = 2;
    cursor(&mut diff, cursor_line, &mut context);
    for file in &mut diff.files {
        if let Some(_expanded_line) = file.expand(cursor_line, &mut context) {
            for child in file.get_children() {
                let view = child.get_view();
                if view.line_no.get() == cursor_line {
                    // hunks were expanded by default.
                    // now they are collapsed!
                    assert!(!view.is_expanded());
                    assert!(view.is_child_dirty());
                    for line in child.get_children() {
                        assert!(line.get_view().is_squashed());
                    }
                }
            }
            break;
        }
    }
}

#[test]
fn test_render_view() {
    initialize();
    let buffer = TextBuffer::new(None);
    let mut iter = buffer.iter_at_line(0).unwrap();
    buffer.insert(&mut iter, "begin\n");
    // -------------------- test insert
    let view1 = View::new();
    let view2 = View::new();
    let view3 = View::new();

    let mut ctx = StatusRenderContext::new();

    view1.render_in_textview(
        &buffer,
        &mut iter,
        "test1".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    view2.render_in_textview(
        &buffer,
        &mut iter,
        "test2".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    view3.render_in_textview(
        &buffer,
        &mut iter,
        "test3".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(view1.line_no.get() == 1);
    assert!(view2.line_no.get() == 2);
    assert!(view3.line_no.get() == 3);
    assert!(view1.is_rendered());
    assert!(view2.is_rendered());
    assert!(view3.is_rendered());
    assert!(iter.line() == 4);
    // ------------------ test rendered in line
    iter = buffer.iter_at_line(1).unwrap();
    view1.render_in_textview(
        &buffer,
        &mut iter,
        "test1".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    view2.render_in_textview(
        &buffer,
        &mut iter,
        "test2".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    view3.render_in_textview(
        &buffer,
        &mut iter,
        "test3".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(iter.line() == 4);

    // ------------------ test deleted
    iter = buffer.iter_at_line(1).unwrap();
    view1.squash(true);
    view1.render(false);

    view1.render_in_textview(
        &buffer,
        &mut iter,
        "test1".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(!view1.is_rendered());
    // its no longer squashed. is it ok?
    assert!(!view1.is_squashed());
    // iter was not moved (nothing to delete, view was not rendered)
    assert!(iter.line() == 1);
    // rerender it
    view1.render_in_textview(
        &buffer,
        &mut iter,
        "test1".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(iter.line() == 2);

    // -------------------- test dirty
    view2.dirty(true);
    view2.render_in_textview(
        &buffer,
        &mut iter,
        "test2".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(!view2.is_dirty());
    assert!(iter.line() == 3);
    // -------------------- test squashed
    view3.squash(true);
    view3.render_in_textview(
        &buffer,
        &mut iter,
        "test3".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(!view3.is_squashed());
    // iter remains on same kine, just squashing view in place
    assert!(iter.line() == 3);
    // -------------------- test transfered
    view3.line_no.replace(0);
    view3.dirty(true);
    view3.transfer(true);
    view3.render_in_textview(
        &buffer,
        &mut iter,
        "test3".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(view3.line_no.get() == 3);
    assert!(view3.is_rendered());
    assert!(!view3.is_dirty());
    assert!(!view3.is_transfered());
    assert!(iter.line() == 4);

    // --------------------- test not in place
    iter = buffer.iter_at_line(3).unwrap();
    view3.line_no.replace(0);
    view3.render_in_textview(
        &buffer,
        &mut iter,
        "test3".to_string(),
        false,
        Vec::new(),
        &mut ctx,
    );
    assert!(view3.line_no.get() == 3);
    assert!(view3.is_rendered());
    assert!(iter.line() == 4);

    // call it here, cause rust creates threads event with --test-threads=1
    // and gtk should be called only from main thread
    test_expand_line();
    // test_reconciliation_new();
}

fn test_expand_line() {
    let buffer = TextBuffer::new(None);
    let mut iter = buffer.iter_at_line(0).unwrap();
    buffer.insert(&mut iter, "begin\n");
    let diff = create_diff();
    let mut ctx = StatusRenderContext::new();
    diff.render(&buffer, &mut iter, &mut ctx);
    // if cursor returns true it need to rerender as in Status!
    if diff.cursor(1, false, &mut ctx) {
        diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), &mut ctx);
    }

    // expand first file
    diff.files[0].expand(1, &mut ctx);
    diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), &mut ctx);

    let content = buffer.slice(&buffer.start_iter(), &buffer.end_iter(), true);
    let content_lines = content.split('\n');

    for (i, cl) in content_lines.enumerate() {
        if i == 0 {
            continue;
        }
        diff.walk_down(&move |vc: &dyn ViewContainer| {
            if vc.get_view().line_no.get() == i as i32 {
                debug!("{:?} - {:?} = {:?}", i, cl, vc.get_content());
                assert!(cl.trim() == vc.get_content());
            }
        });
    }

    let line_of_line = diff.files[0].hunks[0].lines[1].view.line_no.get();
    // put cursor inside first hunk
    if diff.cursor(line_of_line, false, &mut ctx) {
        // if comment out next line the line_of_line will be not sqashed
        diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), &mut ctx);
    }
    // expand on line inside first hunk
    diff.files[0].expand(line_of_line, &mut ctx);
    diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), &mut ctx);

    let content = buffer.slice(&buffer.start_iter(), &buffer.end_iter(), true);
    let content_lines = content.split('\n');
    // ensure that hunk1 is collapsed eg hunk2 follows hunk1 (no lines between)
    let hunk1_content = diff.files[0].hunks[0].get_content();
    let hunk2_content = diff.files[0].hunks[1].get_content();
    let mut hunk1_passed = false;
    for (i, cl) in content_lines.enumerate() {
        debug!("{} {}", i, cl);
        if cl == hunk1_content {
            hunk1_passed = true
        } else if hunk1_passed {
            assert!(cl == hunk2_content);
            hunk1_passed = false;
        }
    }
}

#[test]
fn test_reconciliation_new() {
    initialize();
    let mut context = StatusRenderContext::new();
    let buffer = TextBuffer::new(None);
    let mut iter = buffer.iter_at_line(0).unwrap();

    debug!("............... Case 1.1");

    let mut rendered_file = create_file("File");
    rendered_file.hunks = Vec::new();
    
    for header in [
        "@@ -11,7 +11,8 @@ const path = require('path');",
        "@@ -106,9 +107,9 @@ function getDepsList() {",
        "@@ -128,7 +129,8 @@ function getDepsList() {",
    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        rendered_file.hunks.push(hunk);
    }
    rendered_file.view.expand(true);
    rendered_file.render(&buffer, &mut iter, &mut context);

    // 1.1    
    let mut new_file = create_file("File");
    new_file.hunks = Vec::new();

    for header in [
        "@@ -106,9 +106,9 @@ function getDepsList() {",
        "@@ -128,7 +128,8 @@ function getDepsList() {"
    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        new_file.hunks.push(hunk);
    }
    iter.set_line(0);
    
    new_file.enrich_view(&mut rendered_file, &buffer, &mut context);
    debug!("iter over rendered hunks");
    for (i, h) in rendered_file.hunks.iter().enumerate() {
        debug!("first hunk is squashed, others are rendered {}", h.view.repr());
        if i == 0 {
            // TODO when introduce new mass erase (erase without render) it will need to
            // check other criteria, not rendered!
            assert!(!h.view.is_rendered());
            for line in &h.lines {
                // TODO when introduce new mass erase (erase without render) it will need to
                // check other criteria, not rendered!
                assert!(!line.view.is_rendered());
            }
        } else {
            // TODO when introduce new mass erase (erase without render) it will need to
            // check other criteria, not rendered!
            assert!(h.view.is_rendered());
            for line in &h.lines {
                // TODO when introduce new mass erase (erase without render) it will need to
                // check other criteria, not rendered!
                assert!(line.view.is_rendered());
            }
        }
    }
    debug!("iter over new hunks");
    for h in &new_file.hunks {
        debug!(">>>>>>>>>all new hunks are transfered header: {} view: {}", h.header, h.view.repr());
        debug!("new hunk line len {:?}", h.lines.len());
        assert!(h.view.is_transfered());
        for line in &h.lines {
            debug!("rrrrrrrrrrrrrrrrrrrrrr {:?}", line.content);
            debug!("ooooooooooooooops {:?}", line.view.repr());
            assert!(line.view.is_transfered());
        }
    }

    debug!("2@@@@@@@@@@@@@@@@@@@@@@@@@@@");
    return;

    // --------------------------- 1.2 -----------
    debug!("............... Case 1.2");
    iter = buffer.iter_at_offset(0);
    
    buffer.delete(&mut iter, &mut buffer.end_iter());

    rendered_file.hunks = Vec::new();
    
    for header in [
        "@@ -11,7 +11,8 @@ const path = require('path');",
        "@@ -106,9 +107,9 @@ function getDepsList() {",
        "@@ -128,7 +129,8 @@ function getDepsList() {",
    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        rendered_file.hunks.push(hunk);
    }
    rendered_file.view.expand(true);
    rendered_file.render(&buffer, &mut iter, &mut context);
            
    new_file.hunks = Vec::new();

    for header in [
        "@@ -107,9 +107,9 @@ function getDepsList() {",
        "@@ -129,7 +129,8 @@ function getDepsList() {"

    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        new_file.hunks.push(hunk);
    }

    rendered_file.hunks = Vec::new();
    
    for header in [
        "@@ -11,7 +11,8 @@ const path = require('path');",
        "@@ -106,9 +107,9 @@ function getDepsList() {",
        "@@ -128,7 +129,8 @@ function getDepsList() {",
    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        rendered_file.hunks.push(hunk);
    }
    iter.set_line(0);
    
    rendered_file.view.expand(true);
    rendered_file.render(&buffer, &mut iter, &mut context);

        
    new_file.enrich_view(&mut rendered_file, &buffer, &mut context);

    debug!("iter over rendered hunks");
    for (i, h) in rendered_file.hunks.iter().enumerate() {
        debug!("first hunk is squashed, otheres are rendered {}", h.view.repr());
        if i == 0 {
            // TODO when introduce new mass erase (erase without render) it will need to
            // check other criteria, not rendered!
            assert!(!h.view.is_rendered());
            for line in &h.lines {
                // TODO when introduce new mass erase (erase without render) it will need to
                // check other criteria, not rendered!
                assert!(!line.view.is_rendered());
            }
        } else {
            // TODO when introduce new mass erase (erase without render) it will need to
            // check other criteria, not rendered!
            assert!(h.view.is_rendered());
            for line in &h.lines {
                // TODO when introduce new mass erase (erase without render) it will need to
                // check other criteria, not rendered!
                assert!(line.view.is_rendered());
            }
        }
    }
    debug!("iter over new hunks");
    for h in &new_file.hunks {
        debug!("all new hunks are transfered {}", h.view.repr());
        assert!(h.view.is_transfered())
    }
    
    // case 2.1 ------------------------------ 
    debug!("............... Case 2.1");
    // 2.1
    
    iter = buffer.iter_at_offset(0);    
    buffer.delete(&mut iter, &mut buffer.end_iter());

    rendered_file.hunks = Vec::new();
    for header in [
        "@@ -107,9 +107,9 @@ function getDepsList() {",
        "@@ -129,7 +129,8 @@ function getDepsList() {"
    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        rendered_file.hunks.push(hunk);
    }
    rendered_file.view.expand(true);
    rendered_file.render(&buffer, &mut iter, &mut context);


    let mut new_file = create_file("File");
    new_file.hunks = Vec::new();


    iter.set_line(0);
    for header in [
        "@@ -11,7 +11,8 @@ const path = require('path');",
        "@@ -106,9 +107,9 @@ function getDepsList() {",
        "@@ -128,7 +129,8 @@ function getDepsList() {",
    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        new_file.hunks.push(hunk);
    }
    iter.set_line(0);
    new_file.enrich_view(&mut rendered_file, &buffer, &mut context);
    debug!("iter over rendered hunks");
    for h in &rendered_file.hunks {
        debug!("all hunks are rendered {}", h.view.repr());
        assert!(h.view.is_rendered());
    }
    for (i, h) in new_file.hunks.iter().enumerate() {
        if i == 0 {
            assert!(!h.view.is_transfered())
        } else {
            assert!(h.view.is_transfered())
        }
    }

    // 2.2
    debug!("............... Case 2.2");
    iter = buffer.iter_at_offset(0);    
    buffer.delete(&mut iter, &mut buffer.end_iter());

    rendered_file.hunks = Vec::new();
    for header in [
        "@@ -106,9 +106,9 @@ function getDepsList() {",
        "@@ -128,7 +128,8 @@ function getDepsList() {"

    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        rendered_file.hunks.push(hunk);
    }
    rendered_file.view.expand(true);
    rendered_file.render(&buffer, &mut iter, &mut context);


    let mut new_file = create_file("File");
    new_file.hunks = Vec::new();


    iter.set_line(0);
    for header in [
        "@@ -11,7 +11,8 @@ const path = require('path');",
        "@@ -106,9 +107,9 @@ function getDepsList() {",
        "@@ -128,7 +129,8 @@ function getDepsList() {"

    ] {
        let mut hunk = create_hunk(header);
        hunk.fill_from_header();
        new_file.hunks.push(hunk);
    }
    iter.set_line(0);
    new_file.enrich_view(&mut rendered_file, &buffer, &mut context);
    debug!("iter over rendered hunks");
    for h in &rendered_file.hunks {
        debug!("all hunks are rendered {}", h.view.repr());
        assert!(h.view.is_rendered());
    }
    for (i, h) in new_file.hunks.iter().enumerate() {
        if i == 0 {
            assert!(!h.view.is_transfered())
        } else {
            assert!(h.view.is_transfered())
        }
    }    
}


#[test]
fn test_tags() {
    let tag1 = tags::TxtTag::from_str(tags::TEXT_TAGS[10]);
    let tag3 = tags::TxtTag::from_str(tags::TEXT_TAGS[3]);

    let mut view = View::new();
    view.tag_added(&tag1);
    debug!("added at 1 {:b}", view.tag_indexes.get());
    assert!(view.tag_indexes.get() == tags::TagIdx::from(0b10000000000));
    assert!(view.tag_indexes.get().is_added(&tag1));

    view.tag_added(&tag3);
    debug!("added at 3 {:b}", view.tag_indexes.get());
    assert!(view.tag_indexes.get() == tags::TagIdx::from(0b10000001000));
    assert!(view.tag_indexes.get().is_added(&tag1));
    assert!(view.tag_indexes.get().is_added(&tag3));

    view.tag_removed(&tag1);
    debug!("removed at 1 {:b}", view.tag_indexes.get());
    assert!(view.tag_indexes.get() == tags::TagIdx::from(0b00001000));
    assert!(!view.tag_indexes.get().is_added(&tag1));
    assert!(view.tag_indexes.get().is_added(&tag3));

    view.tag_removed(&tag3);
    // view.tag_indexes.added("tag3");
    debug!("removed at 3 {:b}", view.tag_indexes.get());
    assert!(view.tag_indexes.get() == tags::TagIdx::from(0b00000000));
    assert!(!view.tag_indexes.get().is_added(&tag1));
    assert!(!view.tag_indexes.get().is_added(&tag3));
}

#[test]
pub fn test_line() {
    let mut flags = RenderFlags::new();

    flags = flags.expand(true);
    flags = flags.squash(true);

    debug!(
        "------------- set {:b} {} {}",
        flags,
        flags.is_squashed(),
        flags.is_expanded()
    );
    flags = flags.expand(false);
    debug!(
        "------------- set {:b} {} {}",
        flags,
        flags.is_squashed(),
        flags.is_expanded()
    );
}
