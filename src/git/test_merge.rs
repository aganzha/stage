use log::debug;
use git2;
use std::sync::Once;
use crate::git::{make_diff, DiffKind, MARKER_OURS, MARKER_THEIRS, Hunk, LineKind};
use crate::git::merge::choose_conflict_side_of_blob;


pub const TEST_BLOB: &str = "diff --git a/src/events.py b/src/events.py
index 7f27a52..8d101d8 100644
--- a/src/events.py
+++ b/src/events.py
@@ -100,36 +100,16 @@ _query_has_open_event =
              select true
              from Event e join EventExt ex on e.Id = ex.Id
              WHERE e.Doc = $1::bigint AND
-<<<<<<< HEAD
                  e.Description = any($2::text[])
-=======
-                 -- inflows, outflows and returns
-                 ex.Type = any('{18, 19, 28, 29}'::int[]) and
-                 case when $3::boolean is null then e.Description = any($2::text[])
-                          when $3::boolean is true then ex.ShortState = 10
-                          when $3::boolean is false then ex.ShortState <> 10
-                 end
->>>>>>> 996751f... fix similar events
              LIMIT 1
          )


 def has_events(doc_id, events):
     if not events:
         return False
     return SqlQueryScalar(
         _query_has_open_event,
         doc_id,
         events
     )
-<<<<<<< HEAD
-=======
-    if result:
-        async_write_history_msg(
-            'found similar event for {} {}'.format(event_name, active),
-            HistoryActions.Change,
-            'WarehouseHistory',
-            doc_id
-        )
-    return result
->>>>>>> 996751f... fix similar events
";

static INIT: Once = Once::new();

#[test]
pub fn choose_ours_in_first_side() {
    INIT.call_once(|| {
        env_logger::builder().format_timestamp(None).init();
        // _ = gtk4::init();
    });

    // this is mock diff, which is the result of obtaining
    // diff via diff_tree_to_workdir with reverse=true
    // means we want to kill all workdir changes to get
    // our tree restored as before merge
    let mut git_diff = git2::Diff::from_buffer(TEST_BLOB.as_bytes())
        .expect("cant create diff");
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    let hunk = &diff.files[0].hunks[0];

    // choose first conflict line in OUR side e.g.
    // "e.Description = any($2::text[])"
    let mut our_choosen_line = &hunk.lines[0];
    for l in &hunk.lines {
        if let Some(line_no) = l.old_line_no {
            if  line_no == 104 {
                our_choosen_line = l;
                break;
            }
        }
    }

    let ours_choosed = true;
    let mut hunk_deltas: Vec<(&str, i32)> = Vec::new();
    let conflict_offset_inside_hunk = hunk.get_conflict_offset_by_line(our_choosen_line);

    debug!(
        "{:?} offset {:?} ... {}",
        our_choosen_line.old_line_no,
        conflict_offset_inside_hunk,
        our_choosen_line.content
    );

    let mut new_body = choose_conflict_side_of_blob(
        TEST_BLOB,
        &mut hunk_deltas,
        |line_offset_inside_hunk, hunk_header| {
            line_offset_inside_hunk == conflict_offset_inside_hunk
                &&
                hunk_header == hunk.header
        },
        ours_choosed
    );
    // no first conflict must be resolved to OURS
    // (means no changes at all, BUT second one
    // should remain^ means all MINUS signs where stripepd off
    // and deltans are asjusted to added lines

    // 11 lines where added - whole second conflict
    assert!(hunk_deltas[0] == (&hunk.header, 11));
    let new_header = Hunk::shift_new_start_and_lines(&hunk.header, 0, 11);
    new_body = new_body.replace(&hunk.header, &new_header);

    git_diff = git2::Diff::from_buffer(new_body.as_bytes())
        .expect("cant create diff");

    for line in new_body.lines() {
        debug!("{}", line);
    }
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    let mut first_passed = false;
    for line in &diff.files[0].hunks[0].lines {

        if !first_passed {
            // handle first conflict
            if line.origin != git2::DiffLineType::Context {
                // all non contect lines are deleted!
                // (our_choosen_line is Context)
                assert!(line.origin == git2::DiffLineType::Deletion);
            }
        } else {
            // handle second conflict
            assert!(line.origin != git2::DiffLineType::Deletion);
        }

        if line.content.starts_with(MARKER_THEIRS) {
            first_passed = true;
        }
    }
}
