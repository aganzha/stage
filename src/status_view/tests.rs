use crate::status_view::{StatusRenderContext, ViewContainer};
use crate::{Diff, DiffKind, File, Hunk, Line, View};
use git2::DiffLineType;
use gtk4::prelude::*;
use gtk4::TextBuffer;
use log::debug;

pub fn create_line(name: String) -> Line {
    let mut line = Line {
        content: String::new(),
        origin: DiffLineType::Context,
        view: View::new(),
        new_line_no: None,
        old_line_no: None,
    };
    line.content = name.to_string();
    line
}

pub fn create_hunk(name: String) -> Hunk {
    let mut hunk = Hunk::new();
    hunk.handle_max(&name);
    hunk.header = name.to_string();
    for i in 0..3 {
        let content = format!("{} -> line {}", hunk.header, i);
        hunk.handle_max(&content);
        hunk.lines.push(create_line(content));
    }
    hunk
}

pub fn create_file(name: String) -> File {
    let mut file = File::new();
    file.path = name.to_string().into();
    for i in 0..3 {
        file.hunks
            .push(create_hunk(format!("{} -> hunk {}", name, i)));
    }
    file
}

pub fn create_diff() -> Diff {
    let mut diff = Diff::new(DiffKind::Unstaged);
    for i in 0..3 {
        diff.files.push(create_file(format!("file{}.rs", i)));
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn mock_render_view(
        vc: &mut dyn ViewContainer,
        mut line_no: i32,
    ) -> i32 {
        let view = vc.get_view();
        view.line_no = line_no;
        view.rendered = true;
        view.dirty = false;
        line_no += 1;
        if view.expanded || view.child_dirty {
            for child in vc.get_children() {
                line_no = mock_render_view(child, line_no)
            }
            vc.get_view().child_dirty = false;
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
    pub fn cursor(diff: &mut Diff, line_no: i32) {
        for (_, file) in diff.files.iter_mut().enumerate() {
            file.cursor(line_no, false);
        }
        // some views will be rerenderred cause highlight changes
        mock_render(diff);
    }

    #[test]
    pub fn test_single_diff() {
        let mut diff = create_diff();

        mock_render(&mut diff);

        for cursor_line in 0..3 {
            cursor(&mut diff, cursor_line);

            for (i, file) in diff.files.iter_mut().enumerate() {
                let view = file.get_view();
                if i as i32 == cursor_line {
                    assert!(view.active);
                    assert!(view.current);
                } else {
                    assert!(!view.active);
                    assert!(!view.current);
                }
                assert!(!view.expanded);
            }
        }
        // last line from prev loop
        // the cursor is on it
        let mut cursor_line = 2;
        for file in &mut diff.files {
            if let Some(expanded_line) = file.expand(cursor_line) {
                assert!(file.get_view().child_dirty);
                break;
            }
        }

        mock_render(&mut diff);

        for (i, file) in diff.files.iter_mut().enumerate() {
            let view = file.get_view();
            if i as i32 == cursor_line {
                assert!(view.rendered);
                assert!(view.current);
                assert!(view.active);
                assert!(view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(view.rendered);
                    assert!(view.active);
                    assert!(!view.squashed);
                    assert!(!view.current);
                });
            } else {
                assert!(!view.current);
                assert!(!view.active);
                assert!(!view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(!view.rendered);
                });
            }
        }

        // go 1 line backward
        // end expand it
        cursor_line = 1;
        cursor(&mut diff, cursor_line);

        for file in &mut diff.files {
            if let Some(expanded_line) = file.expand(cursor_line) {
                break;
            }
        }

        mock_render(&mut diff);
        for (i, file) in diff.files.iter_mut().enumerate() {
            let view = file.get_view();
            let j = i as i32;
            if j < cursor_line {
                // all are inactive
                assert!(!view.current);
                assert!(!view.active);
                assert!(!view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(!view.rendered);
                });
            } else if j == cursor_line {
                // all are active
                assert!(view.rendered);
                assert!(view.current);
                assert!(view.active);
                assert!(view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(view.rendered);
                    assert!(view.active);
                    assert!(!view.current);
                });
            } else if j > cursor_line {
                // all are expanded but inactive
                assert!(view.rendered);
                assert!(!view.current);
                assert!(!view.active);
                assert!(view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(view.rendered);
                    assert!(!view.active);
                    assert!(!view.current);
                });
            }
        }

        // go to first hunk of second file
        cursor_line = 2;
        cursor(&mut diff, cursor_line);
        for file in &mut diff.files {
            if let Some(expanded_line) = file.expand(cursor_line) {
                for child in file.get_children() {
                    let view = child.get_view();
                    if view.line_no == cursor_line {
                        // hunks were expanded by default.
                        // now they are collapsed!
                        assert!(!view.expanded);
                        assert!(view.child_dirty);
                        for line in child.get_children() {
                            assert!(line.get_view().squashed);
                        }
                    }
                }
                break;
            }
        }
    }

    use std::sync::Once;

    static INIT: Once = Once::new();

    pub fn initialize() {
        INIT.call_once(|| {
            env_logger::builder().format_timestamp(None).init();
            _ = gtk4::init();
        });
    }

    #[test]
    fn test_render_view() {
        initialize();
        let buffer = TextBuffer::new(None);
        let mut iter = buffer.iter_at_line(0).unwrap();
        buffer.insert(&mut iter, "begin\n");
        // -------------------- test insert
        let mut view1 = View::new();
        let mut view2 = View::new();
        let mut view3 = View::new();

        let ctx = &mut Some(StatusRenderContext::new());

        view1.render(&buffer, &mut iter, "test1".to_string(), Vec::new(), ctx);
        view2.render(&buffer, &mut iter, "test2".to_string(), Vec::new(), ctx);
        view3.render(&buffer, &mut iter, "test3".to_string(), Vec::new(), ctx);
        assert!(view1.line_no == 1);
        assert!(view2.line_no == 2);
        assert!(view3.line_no == 3);
        assert!(view1.rendered);
        assert!(view2.rendered);
        assert!(view3.rendered);
        assert!(iter.line() == 4);
        // ------------------ test rendered in line
        iter = buffer.iter_at_line(1).unwrap();
        view1.render(&buffer, &mut iter, "test1".to_string(), Vec::new(), ctx);
        view2.render(&buffer, &mut iter, "test2".to_string(), Vec::new(), ctx);
        view3.render(&buffer, &mut iter, "test3".to_string(), Vec::new(), ctx);
        assert!(iter.line() == 4);

        // ------------------ test deleted
        iter = buffer.iter_at_line(1).unwrap();
        view1.squashed = true;
        view1.rendered = false;

        view1.render(&buffer, &mut iter, "test1".to_string(), Vec::new(), ctx);
        assert!(!view1.rendered);
        // its no longer squashed. is it ok?
        assert!(!view1.squashed);
        // iter was not moved (nothing to delete, view was not rendered)
        assert!(iter.line() == 1);
        // rerender it
        view1.render(&buffer, &mut iter, "test1".to_string(), Vec::new(), ctx);
        assert!(iter.line() == 2);

        // -------------------- test dirty
        view2.dirty = true;
        view2.render(&buffer, &mut iter, "test2".to_string(), Vec::new(), ctx);
        assert!(!view2.dirty);
        assert!(iter.line() == 3);
        // -------------------- test squashed
        view3.squashed = true;
        view3.render(&buffer, &mut iter, "test3".to_string(), Vec::new(), ctx);
        assert!(!view3.squashed);
        // iter remains on same kine, just squashing view in place
        assert!(iter.line() == 3);
        // -------------------- test transfered
        view3.line_no = 0;
        view3.dirty = true;
        view3.transfered = true;
        view3.render(&buffer, &mut iter, "test3".to_string(), Vec::new(), ctx);
        assert!(view3.line_no == 3);
        assert!(view3.rendered);
        assert!(!view3.dirty);
        assert!(!view3.transfered);
        assert!(iter.line() == 4);

        // --------------------- test not in place
        iter = buffer.iter_at_line(3).unwrap();
        view3.line_no = 0;
        view3.render(&buffer, &mut iter, "test3".to_string(), Vec::new(), ctx);
        assert!(view3.line_no == 3);
        assert!(view3.rendered);
        assert!(iter.line() == 4);
        // call it here, cause rust creates threads event with --test-threads=1
        // and gtk should be called only from main thread
        test_expand_line();
    }

    fn test_expand_line() {
        let buffer = TextBuffer::new(None);
        let mut iter = buffer.iter_at_line(0).unwrap();
        buffer.insert(&mut iter, "begin\n");
        let mut diff = create_diff();
        let ctx = &mut Some(StatusRenderContext::new());
        diff.render(&buffer, &mut iter, ctx);
        // if cursor returns true it need to rerender as in Status!
        if diff.cursor(1, false) {
            diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), ctx);
        }

        // expand first file
        diff.files[0].expand(1);
        diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), ctx);

        let content = buffer.slice(
            &mut buffer.start_iter(),
            &mut buffer.end_iter(),
            true,
        );
        let content_lines = content.split("\n");

        for (i, cl) in content_lines.enumerate() {
            if i == 0 {
                continue;
            }
            diff.walk_down(&mut move |vc: &mut dyn ViewContainer| {
                if vc.get_view().line_no == i as i32 {
                    debug!("{:?} - {:?} = {:?}", i, cl, vc.get_content());
                    assert!(cl.trim() == vc.get_content());
                }
            });
        }

        let line_of_line = diff.files[0].hunks[0].lines[1].view.line_no;
        // put cursor inside first hunk
        if diff.cursor(line_of_line, false) {
            // if comment out next line the line_of_line will be not sqashed
            diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), ctx);
        }
        // expand on line inside first hunk
        diff.files[0].expand(line_of_line);
        diff.render(&buffer, &mut buffer.iter_at_line(1).unwrap(), ctx);

        let content = buffer.slice(
            &mut buffer.start_iter(),
            &mut buffer.end_iter(),
            true,
        );
        let content_lines = content.split("\n");
        // ensure that hunk1 is collapsed eg hunk2 follows hunk1 (no lines between)
        let hunk1_content = diff.files[0].hunks[0].get_content();
        let hunk2_content = diff.files[0].hunks[1].get_content();
        let mut hunk1_passed = false;
        for (i, cl) in content_lines.enumerate() {
            debug!("{} {}", i, cl);
            if cl == hunk1_content {
                hunk1_passed = true
            } else {
                if hunk1_passed {
                    assert!(cl == hunk2_content);
                    hunk1_passed = false;
                }
            }
        }
    }
}
