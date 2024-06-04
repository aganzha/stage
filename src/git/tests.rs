use log::debug;
use git2;
use crate::git::{make_diff, DiffKind, MARKER_OURS};
use crate::git::merge::choose_conflict_side_of_blob;


pub const TEST_BLOB: &str = "diff --git a/src/events.py b/src/events.py
index 7f27a52..8d101d8 100644
--- a/src/events.py
+++ b/src/events.py
@@@ -100,7 -104,12 +100,16 @@@ _query_has_open_event
              select true
              from Event e join EventExt ex on e.Id = ex.Id
              WHERE e.Doc = $1::bigint AND
++<<<<<<< HEAD
 +                e.Description = any($2::text[])
++=======
+                 -- inflows, outflows and returns
+                 ex.Type = any('{18, 19, 28, 29}'::int[]) and
+                 case when $3::boolean is null then e.Description = any($2::text[])
+                          when $3::boolean is true then ex.ShortState = 10
+                          when $3::boolean is false then ex.ShortState <> 10
+                 end
++>>>>>>> 996751f... fix similar events
              LIMIT 1
          )
      '''
@@@ -112,8 -135,17 +121,19 @@@ def has_events(doc_id, events)
      '''
      if not events:
          return False
 -    result = SqlQueryScalar(
 +    return SqlQueryScalar(
          _query_has_open_event,
          doc_id,
 -        events,
 -        active
 +        events
      )
++<<<<<<< HEAD
++=======
+     if result:
+         async_write_history_msg(
+             'found similar event for {} {}'.format(event_name, active),
+             HistoryActions.Change,
+             'WarehouseHistory',
+             doc_id
+         )
+     return result
++>>>>>>> 996751f... fix similar events

";

#[test]
pub fn choose_conflict_side() {
    debug!("eeeeeeeeeeeeeeeeeeeeeeeeeeeee");

    let git_diff = git2::Diff::from_buffer(TEST_BLOB.as_bytes())
        .expect("cant create diff");
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    for f in &diff.files {
        debug!("oooooooooooooooooooooooo -> {:?}", f.path);
        for h in &f.hunks {
            debug!("eeeeeeeeeeeeeeeeeeeeee -> {:?}", h.header);
        }
    }
    // let mut ours_choosed = true;
    // let mut hunk_deltas: Vec<(&str, i32)> = Vec::new();

    // let mut conflict_offset_inside_hunk: i32 = 0;
    // for (i, l) in hunk.lines.iter().enumerate() {
    //     if l.content.starts_with(MARKER_OURS) {
    //         conflict_offset_inside_hunk = i as i32;
    //     }
    //     if l == &line {
    //         break;
    //     }
    // }

    // let mut new_body = choose_conflict_side_of_blob(
    //     TEST_BLOB,
    //     &mut hunk_deltas,
    //     |line_offset_inside_hunk, hunk_header| {
    //         line_offset_inside_hunk == conflict_offset_inside_hunk
    //             &&
    //             hunk_header == reversed_header
    //     },
    //     ours_choosed
    // );
    // for line in new_body.lines() {
    //     debug!("{}", line);
    // }
}
