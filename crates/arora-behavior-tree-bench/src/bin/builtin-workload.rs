fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "synthetic".into());
    let json = arora_behavior_tree_bench::builtins::json(&name)
        .unwrap_or_else(|| panic!("unknown built-in workload: {name}"));
    println!("{json}");
}
