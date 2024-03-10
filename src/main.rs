use rustix::fd::AsRawFd;
use rustix::net;
use std::env;
use std::io;
use std::io::BufReader;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use tempfile::tempdir;

mod config;
mod template;

use config::{Config,has_true};
use template::Template;

fn find_entry(name: &str, file: &Path) -> anyhow::Result<(i32, i32)> {
    for line in std::fs::read_to_string(file)?.lines() {
        let vals: Vec<&str> = line.split(':').collect();
        if vals[0] == name {
            return Ok((vals[1].parse()?, vals[2].parse()?));
        }
    }

    Err(anyhow::Error::msg("cannot find subuid entry"))
}

fn spawn_with_fds(mut com: Command, use_fds: Vec<i32>) -> io::Result<std::process::Child> {
    unsafe {
        com.pre_exec(move || {
            let mut base = 0;
            for fd in &use_fds {
                nix::unistd::dup2(*fd, base)?;
                base += 1;
            }
            libc::close_range(base as u32, u32::MAX, libc::CLOSE_RANGE_CLOEXEC as i32);
            Ok(())
        })
    };

    com.spawn()
}

struct DBusProxy {
    proc: std::process::Child,
    sync_sock: rustix::fd::OwnedFd,
    dbus_sock_path: std::path::PathBuf,
}

fn spawn_dbus_proxy(temp_dir: &Path) -> anyhow::Result<DBusProxy> {
    let dbus_path = temp_dir.join("dbus");
    let mut dbus_proxy = Command::new("xdg-dbus-proxy");
    let dbus_addr = env::var("DBUS_SESSION_BUS_ADDRESS");

    if let Ok(dbus_addr) = dbus_addr {
        let (s0, s1) = net::socketpair(
            net::AddressFamily::UNIX,
            net::SocketType::STREAM,
            net::SocketFlags::empty(),
            None,
        )?;

        dbus_proxy.arg(dbus_addr);
        dbus_proxy.arg(&dbus_path);
        dbus_proxy.arg("--filter");
        dbus_proxy.arg("--talk=org.freedesktop.*");
        let fd_arg = format!("--fd=0");
        dbus_proxy.arg(fd_arg);

        let s0_fdn = s0.as_raw_fd();
        Ok(DBusProxy {
            proc: spawn_with_fds(dbus_proxy, vec![s0_fdn])?,
            sync_sock: s1,
            dbus_sock_path: dbus_path,
        })
    } else {
        return Err(anyhow::Error::msg("unable to connect to dbus"));
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    let arg_dir = std::path::Path::new(&args[1]);

    if !arg_dir.is_dir() {
        return Err(anyhow::Error::msg(format!(
            "{} is not a directory",
            arg_dir.to_str().unwrap()
        )));
    }

    let config_path = arg_dir.join("cch.json");
    let config_file = std::fs::File::open(&config_path)
        .expect(&format!("unable to open {}", config_path.to_str().unwrap()));
    let config: Config = serde_json::from_reader(BufReader::new(config_file)).unwrap();

    let as_uid0 = has_true(&config.as_uid0);
    let temp_dir = tempdir()?;
    let home = home::home_dir().unwrap();

    let mut dbus_proxy: Option<DBusProxy> = None;

    if has_true(&config.desktop_app) {
        if ! has_true(&config.full_dbus) {
            let db = spawn_dbus_proxy(temp_dir.path())?;
            let mut buf = vec![0];
            rustix::io::read(&db.sync_sock, &mut buf)?; // wait for dbus-proxy
            dbus_proxy = Some(db);
        }
    }

    let (info_from_bwrap_rd, info_from_bwrap_wr) = rustix::pipe::pipe()?;
    let (userns_block_to_bwrap_rd, userns_block_to_bwrap_wr) = rustix::pipe::pipe()?;

    let mut bwrap = Command::new("bwrap");
    bwrap.arg("--bind").arg(arg_dir.join("home")).arg(home);
    bwrap.arg("--unshare-all");

    let template = Template::new(&config, arg_dir);

    let mut dev_bind_dirs = Vec::<PathBuf>::new();

    let bind_dirs = [
        "/etc/localtime",
        "/sys",
        "/lib",
        "/usr/lib",
        "/lib64",
        "/usr/lib64",
    ];
    let bind_dirs_desktop = ["/etc/fonts", "/usr/share"];
    let bind_dirs_net = ["/etc/resolv.conf"];

    let mut bind_dirs: Vec<PathBuf> = bind_dirs.map(|s| PathBuf::from_str(s).unwrap()).to_vec();
    if has_true(&config.desktop_app) {
        bind_dirs.extend(bind_dirs_desktop.map(|s| PathBuf::from_str(s).unwrap()));

        let mut wayland_path = PathBuf::from_str(&env::var("XDG_RUNTIME_DIR").unwrap()).unwrap();
        wayland_path.push(env::var("WAYLAND_DISPLAY").unwrap());
        bind_dirs.push(wayland_path);
    }

    if has_true(&config.inherit_path) {
        let path = std::env::var("PATH").unwrap();
        for p in  path.split(':') {
            let p = PathBuf::from_str(p).unwrap();
            if p.is_dir() {
                bind_dirs.push(p);
            }
        }
    }

    if has_true(&config.use_net) {
        bind_dirs.extend(bind_dirs_net.map(|s| PathBuf::from_str(s).unwrap()));
    }

    if let Some(config_binds) = &config.bind {
        bind_dirs.extend(&mut config_binds.iter().map(|s| PathBuf::from_str(s).unwrap()));
    }
    if let Some(config_binds) = &config.dev_bind {
        dev_bind_dirs.extend(&mut config_binds.iter().map(|s| PathBuf::from_str(s).unwrap()));
    }

    for bind_dir in &bind_dirs {
        let bind_dir = template::substitute(bind_dir.to_str().unwrap(), &template);
        bwrap.arg("--bind").arg(&bind_dir).arg(&bind_dir);
    }

    bwrap.arg("--clearenv");
    bwrap.arg("--die-with-parent");
    bwrap.arg("--unshare-user");
    if has_true(&config.use_net) {
        bwrap.arg("--share-net");
    }
    bwrap.arg("--dev").arg("/dev");
    bwrap.arg("--proc").arg("/proc");
    if has_true(&config.desktop_app) {
        bwrap.arg("--dev-bind").arg("/dev").arg("/dev");
    }
    for db in &dev_bind_dirs {
        bwrap.arg("--dev-bind").arg(&db).arg(&db);
    }

    bwrap.arg("--tmpfs").arg("/tmp");
    if let Some(dp) = &dbus_proxy {
        bwrap
            .arg("--bind")
            .arg(&dp.dbus_sock_path)
            .arg(&dp.dbus_sock_path);
        bwrap
            .arg("--setenv")
            .arg("DBUS_SESSION_BUS_ADDRESS")
            .arg("unix:path=".to_owned() + dp.dbus_sock_path.to_str().unwrap());
    }
    if has_true(&config.full_dbus) {
        let mut dbus_path = PathBuf::from_str(&env::var("XDG_RUNTIME_DIR").unwrap()).unwrap();
        dbus_path.push("bus");
        bwrap.arg("--bind").arg(&dbus_path).arg(&dbus_path);
    }


    if let Some(caps) = &config.caps {
        for c in caps {
            bwrap.arg("--cap-add").arg(c);
        }
    }

    if as_uid0 {
        if has_true(&config.inherit_tty) {
            bwrap.arg("--userns-block-fd").arg("3"); //userns_block_to_bwrap_rd.as_raw_fd().to_string()
            bwrap.arg("--info-fd").arg("4"); // info_from_bwrap_wr.as_raw_fd().to_string();
        } else {
            bwrap.arg("--userns-block-fd").arg("0"); //userns_block_to_bwrap_rd.as_raw_fd().to_string()
            bwrap.arg("--info-fd").arg("1"); // info_from_bwrap_wr.as_raw_fd().to_string();
        }
    }

    if !has_true(&config.inherit_tty) {
        bwrap.arg("--new-session");
    }

    let mut inherit_envs = vec!["HOME", "SHELL", "TERM"];

    if has_true(&config.desktop_app) {
        inherit_envs.extend([
            "GTK_IM_MODULE",
            "GDK_IM_MODULE",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "XDG_SESSION_TYPE",
        ]);
    }
    if has_true(&config.full_dbus) {
        inherit_envs.push("DBUS_SESSION_BUS_ADDRESS");
    }

    if has_true(&config.inherit_path) {
        inherit_envs.push("PATH");
    }

    for key in inherit_envs {
        match env::var(key) {
            Ok(v) => {
                bwrap.arg("--setenv").arg(key).arg(v);
            }
            Err(_) => {}
        }
    }

    bwrap.arg(template::substitute(&config.command, &template));

    let mut fds = Vec::new();
    if has_true(&config.inherit_tty) {
        fds.extend([0, 1, 2]);
    }

    fds.push(userns_block_to_bwrap_rd.as_raw_fd());
    fds.push(info_from_bwrap_wr.as_raw_fd());

    let mut child = spawn_with_fds(bwrap, fds).unwrap();

    let info_from_bwrap_rd: std::fs::File = info_from_bwrap_rd.into();
    let info_from_bwrap_rd = BufReader::new(info_from_bwrap_rd);

    drop(info_from_bwrap_wr);
    drop(userns_block_to_bwrap_rd);

    if as_uid0 {
        let v: serde_json::Value = serde_json::from_reader(info_from_bwrap_rd)?;
        let child_pid = &v["child-pid"];

        let user = nix::unistd::User::from_uid(nix::unistd::getuid())?.unwrap();
        let group = nix::unistd::Group::from_gid(nix::unistd::getgid())?.unwrap();

        let subuid = find_entry(&user.name, Path::new("/etc/subuid"))?;
        let subgid = find_entry(&group.name, Path::new("/etc/subgid"))?;

        let mut newuidmap = Command::new("newuidmap");
        newuidmap.arg(child_pid.to_string());

        // 0 map to current uid
        newuidmap.arg("0");
        newuidmap.arg(nix::unistd::getuid().to_string());
        newuidmap.arg("1");

        newuidmap.arg("1");
        newuidmap.arg(subuid.0.to_string());
        newuidmap.arg(subuid.1.to_string());

        spawn_with_fds(newuidmap, vec![]).unwrap().wait()?;

        let mut newgidmap = Command::new("newgidmap");
        newgidmap.arg(child_pid.to_string());

        // 0 map to current uid
        newgidmap.arg("0");
        newgidmap.arg(nix::unistd::getgid().to_string());
        newgidmap.arg("1");

        newgidmap.arg("1");
        newgidmap.arg(subgid.0.to_string());
        newgidmap.arg(subgid.1.to_string());

        spawn_with_fds(newgidmap, vec![]).unwrap().wait()?;

        rustix::io::write(userns_block_to_bwrap_wr, &[1u8])?;
    }
    child.wait().unwrap();

    if let Some(mut dbus_proxy) = dbus_proxy {
        dbus_proxy.proc.kill().unwrap();
        dbus_proxy.proc.wait().unwrap();
    }

    Ok(())
}
