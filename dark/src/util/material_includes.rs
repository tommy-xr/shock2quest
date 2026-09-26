//! Resolve the declarative include subset of render-material files. This does
//! not execute scripts. Paths stay within the installed asset namespace.

const MAX_DEPTH: usize = 16;
const MAX_BYTES: usize = 1024 * 1024;

pub(super) fn expand_material_includes(
    root: &str,
    source: &str,
    mut read: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    fn expand(
        root: &str,
        path: &str,
        source: &str,
        read: &mut impl FnMut(&str) -> Option<String>,
        stack: &mut Vec<String>,
        bytes: &mut usize,
    ) -> Option<String> {
        if stack.len() >= MAX_DEPTH || stack.iter().any(|entry| entry == path) {
            return None;
        }
        *bytes = bytes.checked_add(source.len())?;
        if *bytes > MAX_BYTES {
            return None;
        }
        stack.push(path.to_owned());
        let mut expanded = String::new();
        for line in source.lines() {
            let code = line.split_once("//").map_or(line, |(code, _)| code).trim();
            let mut fields = code.split_whitespace();
            if fields
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("include"))
            {
                let reference = code["include".len()..].trim().trim_matches('"');
                let include = include_path(root, reference)?;
                let content = read(&include)?;
                expanded.push_str(&expand(root, &include, &content, read, stack, bytes)?);
            } else {
                expanded.push_str(line);
                expanded.push('\n');
            }
        }
        stack.pop();
        Some(expanded)
    }
    expand(root, root, source, &mut read, &mut vec![], &mut 0)
}

/// Includes retain the owning material's directory even inside shared files.
/// Shared profiles refer to ../../materials/pass from an obj/txt16 material,
/// rather than relative to the profile's own materials/ directory. Reject
/// traversal above the archive root and absolute paths.
fn include_path(parent: &str, reference: &str) -> Option<String> {
    let reference = reference.replace('\\', "/").to_ascii_lowercase();
    if reference.is_empty() || reference.starts_with('/') || reference.contains(':') {
        return None;
    }
    let directory = parent.rsplit_once('/').map_or("", |(dir, _)| dir);
    let joined = format!("{directory}/{reference}");
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn nested_relative_includes_preserve_order_and_repeated_passes() {
        let files = HashMap::from([
            (
                "materials/profile.inc",
                "fill\ninclude ../../materials/pass/shine.inc\n",
            ),
            ("materials/pass/shine.inc", "shine\n"),
        ]);
        let result = expand_material_includes(
            "obj/txt16/example.mtl",
            "base\nINCLUDE \"../../MATERIALS/profile.inc\" // shared\ninclude ../../materials/pass/shine.inc\nend",
            |name| files.get(name).map(|text| text.to_string()),
        );
        assert_eq!(result.as_deref(), Some("base\nfill\nshine\nshine\nend\n"));
    }

    #[test]
    fn missing_cyclic_and_out_of_root_includes_fail_without_partial_materials() {
        for source in [
            "include missing.inc",
            "include example.mtl",
            "include ../../../outside.inc",
            "include /absolute.inc",
            "include C:\\outside.inc",
        ] {
            assert!(
                expand_material_includes("obj/txt16/example.mtl", source, |_| Some(
                    source.to_owned()
                ))
                .is_none()
            );
        }
        assert!(
            expand_material_includes("obj/txt16/example.mtl", "include missing.inc", |_| None)
                .is_none()
        );
    }

    #[test]
    fn expansion_has_depth_and_total_size_limits() {
        assert!(expand_material_includes("root", &"x".repeat(MAX_BYTES + 1), |_| None).is_none());
        let mut index = 0;
        assert!(
            expand_material_includes("root", "include next", |_| {
                index += 1;
                Some(format!("include file{index}"))
            })
            .is_none()
        );
    }

    #[test]
    fn relative_paths_handle_windows_separators() {
        assert_eq!(
            include_path(
                "obj/txt16/example.mtl",
                "..\\..\\materials\\pass\\shine.inc"
            ),
            Some("materials/pass/shine.inc".to_owned())
        );
    }
}
