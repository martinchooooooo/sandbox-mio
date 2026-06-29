//! Spike: hydration-vs-stream race for the activity-center task registry.
//!
//! Models the open question in 01_ideas.md: between the `list_tasks` snapshot read and the
//! `listen("activity://task-updated")` registration, an emit can be missed or applied out of
//! order. We compare two consumer protocols under EVERY interleaving of (a) where the snapshot
//! is taken in the producer's update sequence and (b) whether the live listener is registered
//! before or after the snapshot, and assert which protocol converges to the producer's final
//! state.
//!
//! Producer model: a single task whose authoritative state walks through monotonically
//! version-stamped updates (version 1..=N). Each update carries (version, status, progress).
//! "Convergence" = the consumer's final folded view equals the producer's last update.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status { Running, Succeeded, Failed }

#[derive(Clone, Copy, Debug)]
struct Update { version: u64, status: Status, progress: u8 }

/// The producer's full authoritative timeline for one task id.
fn producer_timeline() -> Vec<Update> {
    vec![
        Update { version: 1, status: Status::Running,   progress: 0 },
        Update { version: 2, status: Status::Running,   progress: 25 },
        Update { version: 3, status: Status::Running,   progress: 60 },
        Update { version: 4, status: Status::Running,   progress: 90 },
        Update { version: 5, status: Status::Succeeded, progress: 100 },
    ]
}

/// Fold rule shared by both protocols: an update is applied only if its version strictly
/// exceeds the version already recorded for that id (last-writer-by-version-wins).
fn fold(view: &mut HashMap<u64, Update>, id: u64, u: Update) {
    match view.get(&id) {
        Some(cur) if cur.version >= u.version => {} // stale or duplicate: discard
        _ => { view.insert(id, u); }
    }
}

/// PROTOCOL A — naive: take the snapshot, THEN start listening. No version fold on the snapshot
/// (snapshot is treated as ground truth and written directly). Any update emitted strictly
/// between `snapshot_at` and `subscribe_at` is delivered to neither and is lost forever.
///
/// Returns the converged view for a given (snapshot_at, subscribe_at) cut, where both are
/// indices into the timeline meaning "this many updates have been emitted already".
fn protocol_a(snapshot_at: usize, subscribe_at: usize) -> Option<Update> {
    let timeline = producer_timeline();
    let id = 7u64;
    let mut view: HashMap<u64, Update> = HashMap::new();

    // Snapshot = the latest update that had been emitted by snapshot_at, written directly.
    if snapshot_at > 0 {
        view.insert(id, timeline[snapshot_at - 1]);
    }
    // Live listener only catches updates emitted AT OR AFTER subscribe_at.
    for u in timeline.iter().skip(subscribe_at) {
        // naive: append/overwrite unconditionally (no version guard)
        view.insert(id, *u);
    }
    view.get(&id).copied()
}

/// PROTOCOL B — subscribe FIRST, then snapshot, and fold everything by version (snapshot
/// included). The listener is live before the snapshot read, so it can only ever see an update
/// at the same time as or earlier than... no: it sees every update emitted at/after subscribe.
/// Subscribing before snapshotting guarantees no gap: subscribe_at <= snapshot_at.
fn protocol_b(snapshot_at: usize, subscribe_at: usize) -> Option<Update> {
    let timeline = producer_timeline();
    let id = 7u64;
    let mut view: HashMap<u64, Update> = HashMap::new();

    // Buffer live events that arrive while the snapshot request is in flight.
    let live: Vec<Update> = timeline.iter().skip(subscribe_at).copied().collect();

    // The snapshot reflects updates emitted by snapshot_at, folded by version.
    if snapshot_at > 0 {
        fold(&mut view, id, timeline[snapshot_at - 1]);
    }
    // Apply buffered live events by version — order-independent thanks to the fold rule.
    for u in live { fold(&mut view, id, u); }
    view.get(&id).copied()
}

fn main() {
    let timeline = producer_timeline();
    let final_truth = *timeline.last().unwrap();
    let n = timeline.len();

    // Protocol A: only the cuts with NO gap (subscribe_at <= snapshot_at) could ever be safe.
    // Enumerate the realistic racy cuts where subscribe happens AFTER snapshot (subscribe_at >
    // snapshot_at): that is the real-world ordering "call list_tasks, await it, then listen".
    let mut a_lost = 0;
    let mut a_total = 0;
    for snapshot_at in 0..=n {
        for subscribe_at in snapshot_at..=n {
            // subscribe strictly after snapshot => a gap of (subscribe_at - snapshot_at) updates
            a_total += 1;
            let got = protocol_a(snapshot_at, subscribe_at);
            if got.map(|u| u.version) != Some(final_truth.version) {
                a_lost += 1;
            }
        }
    }

    // Protocol B: subscribe BEFORE snapshot => subscribe_at <= snapshot_at, fold by version.
    // Enumerate every such cut AND every delivery order of the buffered live events.
    let mut b_ok = 0;
    let mut b_total = 0;
    for snapshot_at in 0..=n {
        for subscribe_at in 0..=snapshot_at {
            // Try several permutations of live-event delivery order to prove order-independence.
            let live_len = n - subscribe_at;
            for perm in permutations(live_len) {
                b_total += 1;
                let got = protocol_b_perm(snapshot_at, subscribe_at, &perm);
                if got.map(|u| u.version) == Some(final_truth.version) {
                    b_ok += 1;
                } else {
                    println!("PROTOCOL B FAILED at snapshot_at={snapshot_at} subscribe_at={subscribe_at} perm={perm:?} got={got:?}");
                }
            }
        }
    }

    println!("Producer final truth: version={} status={:?} progress={}", final_truth.version, final_truth.status, final_truth.progress);
    println!();
    println!("PROTOCOL A (snapshot-then-subscribe, no version fold):");
    println!("  racy cuts evaluated: {a_total}, cuts that LOST the terminal update: {a_lost}");
    println!("  => loses updates whenever an emit lands in the snapshot->subscribe gap: {}", if a_lost > 0 { "CONFIRMED BUG" } else { "no loss" });
    println!();
    println!("PROTOCOL B (subscribe-then-snapshot, fold by version):");
    println!("  (cut, delivery-order) combinations evaluated: {b_total}, converged to truth: {b_ok}");
    println!("  => converges under every interleaving AND every delivery order: {}", if b_ok == b_total { "CONFIRMED SAFE" } else { "FAILED" });
    assert!(a_lost > 0, "expected protocol A to lose updates");
    assert_eq!(b_ok, b_total, "expected protocol B to always converge");
    println!();
    println!("RESULT: subscribe-before-snapshot + version-stamped fold resolves the race.");
}

/// Protocol B with an explicit delivery order `perm` over the buffered live events.
fn protocol_b_perm(snapshot_at: usize, subscribe_at: usize, perm: &[usize]) -> Option<Update> {
    let timeline = producer_timeline();
    let id = 7u64;
    let mut view: HashMap<u64, Update> = HashMap::new();
    if snapshot_at > 0 {
        fold(&mut view, id, timeline[snapshot_at - 1]);
    }
    let live: Vec<Update> = timeline.iter().skip(subscribe_at).copied().collect();
    for &i in perm {
        fold(&mut view, id, live[i]);
    }
    view.get(&id).copied()
}

/// All permutations of indices 0..len (len kept small: <= 5 here).
fn permutations(len: usize) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..len).collect();
    let mut out = Vec::new();
    permute(&mut idx, 0, &mut out);
    if out.is_empty() { out.push(vec![]); }
    out
}
fn permute(a: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
    if k == a.len() { out.push(a.clone()); return; }
    for i in k..a.len() {
        a.swap(k, i);
        permute(a, k + 1, out);
        a.swap(k, i);
    }
}
