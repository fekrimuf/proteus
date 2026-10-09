//! Phase 2 gate: the recipe set builds from hcs, passes its specs and the static checker,
//! cooks to the recorded golden hashes, and is independent of thread count.

use pt_cli::recipes;

fn run_all() -> Vec<recipes::RecipeResult> {
    recipes::run_all(false, None).expect("recipes run")
}

#[test]
fn all_recipes_pass_specs_and_golden_hashes() {
    let results = run_all();
    assert!(results.len() >= 10, "need the classic ten, found {}", results.len());
    for r in &results {
        assert!(r.failures.is_empty(), "{}: {:?}", r.name, r.failures);
        assert!(r.specs_total >= 3, "{} must declare specs", r.name);
        assert_eq!(r.specs_ok, r.specs_total);
        assert!(!r.hash.is_empty());
    }
}

#[test]
fn recipe_hashes_do_not_depend_on_thread_count() {
    let hashes = |threads: usize| -> Vec<String> {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| run_all().into_iter().map(|r| r.hash).collect())
    };
    assert_eq!(hashes(1), hashes(6));
}

#[test]
fn recipes_exercise_the_engine_breadth() {
    use std::collections::BTreeSet;
    let mut kinds = BTreeSet::new();
    for p in recipes::list() {
        let code = std::fs::read_to_string(&p).unwrap();
        let mut g = pt_graph::Graph::new();
        pt_hcs::run(&code, &mut g, true).unwrap();
        kinds.extend(g.nodes.values().map(|n| n.kind.clone()));
    }
    for want in ["wrangle", "array", "radial_array", "copy_to_points", "scatter_poisson", "subdivide", "polyextrude", "hda", "group_by_expression", "blast", "smooth", "normals", "center"] {
        assert!(kinds.contains(want), "no recipe uses '{want}' (used: {kinds:?})");
    }
}
