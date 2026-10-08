use std::{env, fs, path::Path};

fn main() {
    let release = env::var("PROFILE").as_deref() == Ok("release");
    let root = if !release && Path::new("../web/dist-debug/index.html").exists() {
        Path::new("../web/dist-debug")
    } else {
        Path::new("../web/dist")
    };
    println!("cargo:rerun-if-changed=../web/dist");
    println!("cargo:rerun-if-changed=../web/dist-debug");
    let mut files = Vec::new();
    if root.exists() {
        collect(root, root, &mut files);
    }
    if release && !root.join("index.html").exists() {
        panic!("Build the frontend first: cd web && pnpm install && pnpm build");
    }
    files.sort();
    let mut code = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for (url, path) in files {
        code.push_str(&format!("({url:?}, include_bytes!({path:?})),\n"));
    }
    code.push_str("];\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("assets.rs"),
        code,
    )
    .unwrap();
}

fn collect(root: &Path, path: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            files.push((
                format!(
                    "/{}",
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                ),
                fs::canonicalize(path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ));
        }
    }
}
