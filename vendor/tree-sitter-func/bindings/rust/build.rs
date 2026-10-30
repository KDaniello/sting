fn main() {
    let src_dir = std::path::Path::new("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(src_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    // No external scanner in this grammar today (grammar.js has no
    // `externals`) — but if a future grammar.js edit adds one, `tree-sitter
    // generate` would emit src/scanner.c and this build would need to
    // compile it too, same as tree-sitter-tolk's own build.rs already does.
    // Checking unconditionally now means that day doesn't require anyone to
    // remember to come back and add this.
    let scanner_path = src_dir.join("scanner.c");
    if scanner_path.exists() {
        c_config.file(&scanner_path);
        println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-func");
}
