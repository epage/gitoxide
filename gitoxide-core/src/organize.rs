use bstr::ByteSlice;
use git_features::progress::Progress;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Mode {
    Execute,
    Simulate,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Simulate
    }
}

// TODO: handle nested repos, skip everything inside a parent directory, stop recursing into git workdirs
fn find_git_repository_workdirs<P: Progress>(root: impl AsRef<Path>, mut progress: P) -> impl Iterator<Item = PathBuf>
where
    <P as Progress>::SubProgress: Sync,
{
    progress.init(None, git_features::progress::count("filesystem items"));
    fn is_repository(path: &PathBuf) -> bool {
        if !(path.is_dir() && path.ends_with(".git")) {
            return false;
        }
        path.join("HEAD").is_file() && path.join("config").is_file()
    }
    fn into_workdir(path: PathBuf) -> PathBuf {
        fn is_bare(path: &Path) -> bool {
            !path.join("index").exists()
        }
        if is_bare(&path) {
            path
        } else {
            path.parent().expect("git is never in the root").to_owned()
        }
    }

    let walk = jwalk::WalkDirGeneric::<((), bool)>::new(root)
        .follow_links(false)
        .sort(false)
        .skip_hidden(false);

    // On macos with apple silicon, the IO subsystem is entirely different and one thread can mostly max it out.
    // Thus using more threads just burns energy unnecessarily.
    // It's notable that `du` is very fast even on a single core and more power efficient than dua with a single core.
    // The default of '4' seems related to the amount of performance cores present in the system.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let walk = walk.parallelism(jwalk::Parallelism::RayonNewPool(4));

    walk.process_read_dir(move |_depth, _path, _read_dir_state, children| {
        for entry in children.iter_mut() {
            if let Ok(e) = entry {
                if is_repository(&e.path()) {
                    e.client_state = true;
                    e.read_children_path = None;
                }
            }
        }
    })
    .into_iter()
    .inspect(move |_| progress.inc())
    .filter_map(Result::ok)
    .filter(|e| e.client_state)
    .map(|e| into_workdir(e.path()))
}

fn find_origin_remote(repo: &Path) -> anyhow::Result<Option<git_url::Url>> {
    let out = std::process::Command::new("git")
        .args(&["remote", "--verbose"])
        .current_dir(repo)
        .output()?;
    if out.status.success() {
        Ok(parse::remotes_from_git_remote_verbose(&out.stdout)?
            .into_iter()
            .find_map(|(origin, url)| if origin == "origin" { Some(url) } else { None }))
    } else {
        anyhow::bail!(
            "git invocation failed with code {:?}: {}",
            out.status.code(),
            out.stderr.as_bstr()
        )
    }
}

fn handle(
    mode: Mode,
    git_workdir: &Path,
    canonicalized_destination: &Path,
    progress: &mut impl Progress,
) -> anyhow::Result<()> {
    fn to_relative(path: PathBuf) -> PathBuf {
        path.components()
            .skip_while(|c| c == &std::path::Component::RootDir)
            .collect()
    }

    let url = match find_origin_remote(git_workdir)? {
        None => {
            progress.info(format!(
                "Skipping repository {:?} without 'origin' remote",
                git_workdir.display()
            ));
            return Ok(());
        }
        Some(url) => url,
    };
    if url.path.is_empty() {
        progress.info(format!(
            "Skipping repository at {:?} whose remote does not have a path: {:?}",
            git_workdir.display(),
            url.to_string()
        ));
        return Ok(());
    }

    let destination = canonicalized_destination
        .join(
            url.host
                .as_ref()
                .ok_or_else(|| anyhow::Error::msg(format!("Remote URLs must have host names: {}", url)))?,
        )
        .join(to_relative(git_url::expand_path(None, url.path.as_bstr())?));
    match mode {
        Mode::Simulate => progress.info(format!(
            "WOULD move {} to {}",
            git_workdir.display(),
            destination.display()
        )),
        Mode::Execute => {
            if git_workdir.canonicalize()? == destination {
                progress.info(format!(
                    "Skipping {:?} as it is in the correct spot",
                    git_workdir.display()
                ));
            } else {
                std::fs::create_dir_all(destination.parent().expect("repo destination is not the root"))?;
                progress.info(format!("Moving {} to {}", git_workdir.display(), destination.display()));
                std::fs::rename(git_workdir, &destination)?;
            }
        }
    }
    Ok(())
}

pub fn run<P: Progress>(mode: Mode, source_dir: PathBuf, destination: PathBuf, mut progress: P) -> anyhow::Result<()>
where
    <<P as Progress>::SubProgress as Progress>::SubProgress: Sync,
{
    let search_progress = progress.add_child("Searching repositories");

    let mut num_errors = 0usize;
    let destination = destination.canonicalize()?;
    for path_to_move in find_git_repository_workdirs(source_dir, search_progress) {
        if let Err(err) = handle(mode, &path_to_move, &destination, &mut progress) {
            progress.fail(format!(
                "Error when handling directory {:?}: {}",
                path_to_move.display(),
                err.to_string()
            ));
            num_errors += 1;
        }
    }

    if num_errors > 0 {
        anyhow::bail!("Failed to handle {} repositories", num_errors)
    } else {
        Ok(())
    }
}

mod parse {
    use anyhow::Context;
    use bstr::{BStr, ByteSlice};

    #[allow(unused)]
    pub fn remotes_from_git_remote_verbose(input: &[u8]) -> anyhow::Result<Vec<(&BStr, git_url::Url)>> {
        fn parse_line(line: &BStr) -> anyhow::Result<(&BStr, git_url::Url)> {
            let mut tokens = line.splitn(2, |b| *b == b'\t');
            Ok(match (tokens.next(), tokens.next(), tokens.next()) {
                (Some(remote), Some(url_and_type), None) => {
                    let mut tokens = url_and_type.splitn(2, |b| *b == b' ');
                    match (tokens.next(), tokens.next(), tokens.next()) {
                        (Some(url), Some(_type), None) => (remote.as_bstr(), git_url::parse(url)?),
                        _ => anyhow::bail!("None or more than one 'space' as separator"),
                    }
                }
                _ => anyhow::bail!("None or more than one tab as separator"),
            })
        }

        let mut out = Vec::new();
        for line in input.split(|b| *b == b'\n') {
            let line = line.as_bstr();
            if line.trim().is_empty() {
                continue;
            }
            out.push(
                parse_line(line).with_context(|| format!("Line {:?} should be <origin>TAB<URL>SPACE<TYPE>", line))?,
            );
        }

        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bstr::ByteSlice;

        static GITOXIDE_REMOTES: &[u8] = br#"commitgraph	https://github.com/avoidscorn/gitoxide (fetch)
commitgraph	https://github.com/avoidscorn/gitoxide (push)
origin	https://github.com/Byron/gitoxide (fetch)
origin	https://github.com/Byron/gitoxide (push)
rad	rad://hynkuwzskprmswzeo4qdtku7grdrs4ffj3g9tjdxomgmjzhtzpqf81@hwd1yregyf1dudqwkx85x5ps3qsrqw3ihxpx3ieopq6ukuuq597p6m8161c.git (fetch)
rad	rad://hynkuwzskprmswzeo4qdtku7grdrs4ffj3g9tjdxomgmjzhtzpqf81@hwd1yregyf1dudqwkx85x5ps3qsrqw3ihxpx3ieopq6ukuuq597p6m8161c.git (push)
"#;
        fn url(input: &str) -> git_url::Url {
            git_url::Url::from_bytes(input.as_bytes()).expect("valid url")
        }

        #[test]
        fn valid_verbose_remotes() -> anyhow::Result<()> {
            assert_eq!(
                remotes_from_git_remote_verbose(GITOXIDE_REMOTES)?,
                vec![
                    (b"commitgraph".as_bstr(), url("https://github.com/avoidscorn/gitoxide")),
                    (b"commitgraph".as_bstr(), url("https://github.com/avoidscorn/gitoxide")),
                    (b"origin".as_bstr(), url("https://github.com/Byron/gitoxide")),
                    (b"origin".as_bstr(), url("https://github.com/Byron/gitoxide")),
                    (
                        b"rad".as_bstr(),
                        url("rad://hynkuwzskprmswzeo4qdtku7grdrs4ffj3g9tjdxomgmjzhtzpqf81@hwd1yregyf1dudqwkx85x5ps3qsrqw3ihxpx3ieopq6ukuuq597p6m8161c.git")
                    ),
                    (
                        b"rad".as_bstr(),
                        url("rad://hynkuwzskprmswzeo4qdtku7grdrs4ffj3g9tjdxomgmjzhtzpqf81@hwd1yregyf1dudqwkx85x5ps3qsrqw3ihxpx3ieopq6ukuuq597p6m8161c.git")
                    )
                ]
            );
            Ok(())
        }
    }
}
