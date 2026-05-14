use std::path::{Component, PathBuf};

use crate::protocol::types::FileInfo;

/// Parse a comma-separated list of u64 values from a query parameter string.
#[must_use]
pub fn parse_csv_u64(s: &str) -> Vec<u64> {
    s.split(',')
        .filter_map(|v| v.trim().parse::<u64>().ok())
        .collect()
}

/// Parse a CSV-encoded job steps string into (what, state) pairs.
/// Format: "what1,state1,what2,state2,..."
/// Returns a Vec to preserve duplicate `what` values (e.g. same source, different states).
#[must_use]
pub fn parse_job_steps(s: &str) -> Vec<(String, u32)> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i + 1 < parts.len() {
        if let Ok(state) = parts[i + 1].parse::<u32>() {
            result.push((parts[i].to_string(), state));
        }
        i += 2;
    }
    result
}

/// Canonicalize a file path (resolve ".." and "." without filesystem access).
fn weak_canonical(path: &str) -> String {
    let p = PathBuf::from(path);
    let mut result = PathBuf::new();
    for component in p.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result.to_string_lossy().to_string()
}

/// Filters a list of files by path and recursion flag, canonicalising the target path before matching.
#[must_use]
pub fn filter_files(files: &[FileInfo], file_path: &str, recursive: bool) -> Vec<FileInfo> {
    // Get the canonical path with leading and trailing slash
    let mut abs_path = weak_canonical(file_path);

    // Ensure leading slash
    if abs_path.is_empty() || !abs_path.starts_with('/') {
        abs_path = format!("/{abs_path}");
    }

    // Ensure trailing slash
    if !abs_path.ends_with('/') {
        abs_path.push('/');
    }

    // Version without trailing slash for matching directories themselves
    let abs_path_no_trail = abs_path.trim_end_matches('/').to_string();

    let mut matched = Vec::new();

    for file in files {
        let normalized_file_name = if file.file_name.starts_with('/') {
            file.file_name.clone()
        } else {
            format!("/{}", file.file_name.trim_start_matches('/'))
        };
        let path = PathBuf::from(&normalized_file_name);

        if recursive {
            let parent_path = path
                .parent()
                .map(|p| {
                    let s = p.to_string_lossy().to_string();
                    if !s.is_empty() && !s.ends_with('/') {
                        format!("{s}/")
                    } else {
                        s
                    }
                })
                .unwrap_or_default();

            if normalized_file_name == abs_path
                || parent_path.starts_with(&abs_path)
                || (normalized_file_name == abs_path_no_trail && file.is_directory)
            {
                matched.push(file.clone());
            }
        } else {
            let parent = path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            if parent == abs_path
                || parent == abs_path_no_trail
                || (normalized_file_name == abs_path_no_trail && file.is_directory)
            {
                matched.push(file.clone());
            }
        }
    }

    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that `parse_csv_u64` correctly splits a comma-separated string into a vector of
    /// u64 values, ignoring empty segments and non-numeric tokens.
    #[test]
    fn test_parse_csv_u64() {
        assert_eq!(parse_csv_u64("1,2,3"), vec![1, 2, 3]);
        assert_eq!(parse_csv_u64("42"), vec![42]);
        assert!(parse_csv_u64("").is_empty());
        assert_eq!(parse_csv_u64("1,,3"), vec![1, 3]);
        assert_eq!(parse_csv_u64("abc,2,def"), vec![2]);
    }

    /// Verifies that `parse_job_steps` parses a well-formed `what,state,what,state` string
    /// into the expected (what, state) pairs.
    #[test]
    fn test_parse_job_steps() {
        let steps = parse_job_steps("jid0,500,jid1,500");
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().any(|(w, s)| w == "jid0" && *s == 500));
        assert!(steps.iter().any(|(w, s)| w == "jid1" && *s == 500));
    }

    /// Verifies that duplicate `what` values with different states are both preserved
    /// rather than deduplicated.
    #[test]
    fn test_parse_job_steps_duplicate_what_preserved() {
        // Same what key with different states — both must be preserved (not deduplicated)
        let steps = parse_job_steps("system,4,system,9");
        assert_eq!(steps.len(), 2, "duplicate what values must be kept");
        assert!(steps.iter().any(|(w, s)| w == "system" && *s == 4));
        assert!(steps.iter().any(|(w, s)| w == "system" && *s == 9));
    }

    /// Verifies that a trailing unpaired `what` token is ignored while the preceding valid pair
    /// is returned.
    #[test]
    fn test_parse_job_steps_odd() {
        let steps = parse_job_steps("jid0,500,jid1");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0], ("jid0".to_string(), 500));
    }

    /// Verifies that an empty input string yields an empty result.
    #[test]
    fn test_parse_job_steps_empty() {
        let steps = parse_job_steps("");
        assert!(steps.is_empty());
    }

    /// Verifies that `weak_canonical` resolves `.` and `..` path components without filesystem access.
    #[test]
    fn test_weak_canonical() {
        assert_eq!(
            weak_canonical("/test/../test2/test/../myfile"),
            "/test2/myfile"
        );
        assert_eq!(weak_canonical("/a/b/c"), "/a/b/c");
        assert_eq!(weak_canonical(""), "");
        assert_eq!(weak_canonical("/"), "/");
    }

    fn make_files() -> Vec<FileInfo> {
        vec![
            FileInfo {
                file_name: "/project".to_string(),
                file_size: 0,
                permissions: 0o755,
                is_directory: true,
            },
            FileInfo {
                file_name: "/project/file1.txt".to_string(),
                file_size: 100,
                permissions: 0o644,
                is_directory: false,
            },
            FileInfo {
                file_name: "/project/subdir".to_string(),
                file_size: 0,
                permissions: 0o755,
                is_directory: true,
            },
            FileInfo {
                file_name: "/project/subdir/file2.txt".to_string(),
                file_size: 200,
                permissions: 0o644,
                is_directory: false,
            },
            FileInfo {
                file_name: "/other/file3.txt".to_string(),
                file_size: 300,
                permissions: 0o644,
                is_directory: false,
            },
        ]
    }

    fn make_relative_files() -> Vec<FileInfo> {
        vec![
            FileInfo {
                file_name: "job.sh".to_string(),
                file_size: 100,
                permissions: 0o644,
                is_directory: false,
            },
            FileInfo {
                file_name: "subdir".to_string(),
                file_size: 0,
                permissions: 0o755,
                is_directory: true,
            },
            FileInfo {
                file_name: "subdir/output.txt".to_string(),
                file_size: 200,
                permissions: 0o644,
                is_directory: false,
            },
        ]
    }

    /// Verifies that non-recursive filtering returns the target directory and its immediate
    /// children only, excluding nested descendants.
    #[test]
    fn test_filter_files_non_recursive() {
        let files = make_files();
        let result = filter_files(&files, "/project", false);
        let names: Vec<&str> = result.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"/project"));
        assert!(names.contains(&"/project/file1.txt"));
        assert!(names.contains(&"/project/subdir"));
        assert!(!names.contains(&"/project/subdir/file2.txt"));
        assert!(!names.contains(&"/other/file3.txt"));
    }

    /// Verifies that recursive filtering returns the target directory and all nested descendants.
    #[test]
    fn test_filter_files_recursive() {
        let files = make_files();
        let result = filter_files(&files, "/project", true);
        let names: Vec<&str> = result.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"/project"));
        assert!(names.contains(&"/project/file1.txt"));
        assert!(names.contains(&"/project/subdir"));
        assert!(names.contains(&"/project/subdir/file2.txt"));
        assert!(!names.contains(&"/other/file3.txt"));
    }

    /// Verifies that recursive filtering from root `/` returns every file in the list.
    #[test]
    fn test_filter_files_root_recursive() {
        let files = make_files();
        let result = filter_files(&files, "/", true);
        assert_eq!(result.len(), files.len());
    }

    /// Verifies that an empty path is treated as root, returning all files when recursive.
    #[test]
    fn test_filter_files_empty_path_recursive() {
        let files = make_files();
        let result = filter_files(&files, "", true);
        assert_eq!(result.len(), files.len());
    }

    /// Verifies that recursive filtering from an empty path treats cached relative
    /// file paths as rooted under `/`, returning the full completed-job cache.
    #[test]
    fn test_filter_files_empty_path_recursive_with_relative_paths() {
        let files = make_relative_files();
        let result = filter_files(&files, "", true);
        let names: Vec<&str> = result.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"job.sh"));
        assert!(names.contains(&"subdir"));
        assert!(names.contains(&"subdir/output.txt"));
        assert_eq!(result.len(), files.len());
    }

    /// Verifies that non-recursive filtering from an empty path returns only
    /// top-level relative cached entries and excludes nested descendants.
    #[test]
    fn test_filter_files_empty_path_non_recursive_with_relative_paths() {
        let files = make_relative_files();
        let result = filter_files(&files, "", false);
        let names: Vec<&str> = result.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"job.sh"));
        assert!(names.contains(&"subdir"));
        assert!(!names.contains(&"subdir/output.txt"));
    }

    /// Verifies that paths containing `..` are canonicalised before matching.
    #[test]
    fn test_filter_files_with_dotdot() {
        let files = make_files();
        let result = filter_files(&files, "/project/../project", true);
        let names: Vec<&str> = result.iter().map(|f| f.file_name.as_str()).collect();
        assert!(names.contains(&"/project/file1.txt"));
        assert!(names.contains(&"/project/subdir/file2.txt"));
    }

    /// Verifies that filtering against a non-existent directory path returns an empty result.
    #[test]
    fn test_filter_files_no_match() {
        let files = make_files();
        let result = filter_files(&files, "/nonexistent", false);
        assert!(result.is_empty());
    }

    // -------------------------------------------------------------------
    // Extended filter tests covering recursive and non-recursive paths
    // -------------------------------------------------------------------

    /// Build the shared file list used by the extended filter tests.
    fn cpp_file_list() -> Vec<FileInfo> {
        vec![
            FileInfo {
                file_name: "/".to_string(),
                file_size: 0,
                permissions: 0,
                is_directory: true,
            },
            FileInfo {
                file_name: "/test".to_string(),
                file_size: 1234,
                permissions: 0,
                is_directory: false,
            },
            FileInfo {
                file_name: "/testdir".to_string(),
                file_size: 0,
                permissions: 0,
                is_directory: true,
            },
            FileInfo {
                file_name: "/testdir/file".to_string(),
                file_size: 2345,
                permissions: 0,
                is_directory: false,
            },
            FileInfo {
                file_name: "/testdir/file2".to_string(),
                file_size: 3456,
                permissions: 0,
                is_directory: false,
            },
            FileInfo {
                file_name: "/testdir/file3".to_string(),
                file_size: 4567,
                permissions: 0,
                is_directory: false,
            },
            FileInfo {
                file_name: "/testdir/testdir1".to_string(),
                file_size: 0,
                permissions: 0,
                is_directory: true,
            },
            FileInfo {
                file_name: "/testdir/testdir1/file".to_string(),
                file_size: 5678,
                permissions: 0,
                is_directory: false,
            },
            FileInfo {
                file_name: "/test2".to_string(),
                file_size: 6789,
                permissions: 0,
                is_directory: false,
            },
        ]
    }

    fn names(files: &[FileInfo]) -> Vec<&str> {
        files.iter().map(|f| f.file_name.as_str()).collect()
    }

    // --- Relative path recursive ---

    /// Verifies that relative paths containing `..` that resolve to root return the full file list.
    #[test]
    fn test_cpp_relative_path_recursive_dotdot_resolves_to_root() {
        let files = cpp_file_list();

        // /testdir/.. → /  (recursive, should return all)
        let result = filter_files(&files, "/testdir/..", true);
        assert_eq!(names(&result), names(&files));

        let result = filter_files(&files, "/testdir/../", true);
        assert_eq!(names(&result), names(&files));

        let result = filter_files(
            &files,
            "/testdir/../test2/not/real/../../../testdir/..",
            true,
        );
        assert_eq!(names(&result), names(&files));

        let result = filter_files(
            &files,
            "/testdir/../test2/not/real/../../../testdir/../",
            true,
        );
        assert_eq!(names(&result), names(&files));
    }

    /// Verifies that relative paths containing `..` that resolve to `/testdir` return only
    /// that subtree.
    #[test]
    fn test_cpp_relative_path_recursive_dotdot_resolves_to_testdir() {
        let files = cpp_file_list();

        // /testdir/../test2/not/real/../../../testdir → /testdir (recursive)
        let expected_names = vec![
            "/testdir",
            "/testdir/file",
            "/testdir/file2",
            "/testdir/file3",
            "/testdir/testdir1",
            "/testdir/testdir1/file",
        ];

        let result = filter_files(&files, "/testdir/../test2/not/real/../../../testdir", true);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "/testdir/../test2/not/real/../../../testdir/", true);
        assert_eq!(names(&result), expected_names);
    }

    // --- Absolute path recursive ---

    /// Verifies that recursive filtering from root (empty string and `/`) returns all files.
    #[test]
    fn test_cpp_absolute_path_recursive_root() {
        let files = cpp_file_list();

        let result = filter_files(&files, "", true);
        assert_eq!(names(&result), names(&files));

        let result = filter_files(&files, "/", true);
        assert_eq!(names(&result), names(&files));
    }

    /// Verifies that recursive filtering from `/testdir` returns the directory and all its
    /// descendants.
    #[test]
    fn test_cpp_absolute_path_recursive_testdir() {
        let files = cpp_file_list();
        let expected_names = vec![
            "/testdir",
            "/testdir/file",
            "/testdir/file2",
            "/testdir/file3",
            "/testdir/testdir1",
            "/testdir/testdir1/file",
        ];

        let result = filter_files(&files, "/testdir", true);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "/testdir/", true);
        assert_eq!(names(&result), expected_names);
    }

    /// Verifies that recursive filtering from `/testdir/testdir1` works with and without a leading
    /// slash.
    #[test]
    fn test_cpp_absolute_path_recursive_testdir1() {
        let files = cpp_file_list();
        let expected_names = vec!["/testdir/testdir1", "/testdir/testdir1/file"];

        let result = filter_files(&files, "/testdir/testdir1", true);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "/testdir/testdir1/", true);
        assert_eq!(names(&result), expected_names);

        // Without leading slash
        let result = filter_files(&files, "testdir/testdir1", true);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "testdir/testdir1/", true);
        assert_eq!(names(&result), expected_names);
    }

    // --- Absolute path non-recursive ---

    /// Verifies that non-recursive filtering from root returns only root-level entries.
    #[test]
    fn test_cpp_absolute_path_non_recursive_root() {
        let files = cpp_file_list();
        let expected_names = vec!["/", "/test", "/testdir", "/test2"];

        let result = filter_files(&files, "", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "/", false);
        assert_eq!(names(&result), expected_names);
    }

    /// Verifies that non-recursive filtering from `/testdir` works with and without leading or
    /// trailing slashes.
    #[test]
    fn test_cpp_absolute_path_non_recursive_testdir() {
        let files = cpp_file_list();
        let expected_names = vec![
            "/testdir",
            "/testdir/file",
            "/testdir/file2",
            "/testdir/file3",
            "/testdir/testdir1",
        ];

        let result = filter_files(&files, "/testdir", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "/testdir/", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "testdir", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "testdir/", false);
        assert_eq!(names(&result), expected_names);
    }

    /// Verifies that non-recursive filtering from `/testdir/testdir1` works with and without
    /// leading or trailing slashes.
    #[test]
    fn test_cpp_absolute_path_non_recursive_testdir1() {
        let files = cpp_file_list();
        let expected_names = vec!["/testdir/testdir1", "/testdir/testdir1/file"];

        let result = filter_files(&files, "/testdir/testdir1", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "/testdir/testdir1/", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "testdir/testdir1", false);
        assert_eq!(names(&result), expected_names);

        let result = filter_files(&files, "testdir/testdir1/", false);
        assert_eq!(names(&result), expected_names);
    }
}
