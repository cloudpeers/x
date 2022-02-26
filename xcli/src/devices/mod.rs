use crate::devices::adb::Adb;
use crate::devices::host::Host;
use crate::devices::imd::IMobileDevice;
use crate::{Arch, BuildEnv, Platform};
use anyhow::Result;
use std::path::Path;
use std::process::Child;

mod adb;
mod host;
mod imd;

#[derive(Clone, Debug)]
enum Backend {
    Adb(Adb),
    Imd(IMobileDevice),
    Host(Host),
}

#[derive(Clone, Debug)]
pub struct Device {
    backend: Backend,
    id: String,
}

impl std::str::FromStr for Device {
    type Err = anyhow::Error;

    fn from_str(device: &str) -> Result<Self> {
        if device == "host" {
            return Ok(Self::host());
        }
        if let Some((backend, id)) = device.split_once(':') {
            let backend = match backend {
                "adb" => Backend::Adb(Adb::which()?),
                "imd" => Backend::Imd(IMobileDevice::which()?),
                _ => anyhow::bail!("unsupported backend {}", backend),
            };
            Ok(Self {
                backend,
                id: id.to_string(),
            })
        } else {
            anyhow::bail!("invalid device identifier {}", device);
        }
    }
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.backend {
            Backend::Adb(_) => write!(f, "adb:{}", &self.id),
            Backend::Host(_) => write!(f, "{}", &self.id),
            Backend::Imd(_) => write!(f, "imd:{}", &self.id),
        }
    }
}

impl Device {
    pub fn list() -> Result<Vec<Self>> {
        let mut devices = vec![Self::host()];
        if let Ok(adb) = Adb::which() {
            adb.devices(&mut devices)?;
        }
        if let Ok(imd) = IMobileDevice::which() {
            imd.devices(&mut devices)?;
        }
        Ok(devices)
    }

    pub fn host() -> Self {
        Self {
            backend: Backend::Host(Host),
            id: "host".to_string(),
        }
    }

    pub fn is_host(&self) -> bool {
        if let Backend::Host(_) = &self.backend {
            true
        } else {
            false
        }
    }

    pub fn name(&self) -> Result<String> {
        match &self.backend {
            Backend::Adb(adb) => adb.name(&self.id),
            Backend::Host(host) => host.name(),
            Backend::Imd(imd) => imd.name(&self.id),
        }
    }

    pub fn platform(&self) -> Result<Platform> {
        match &self.backend {
            Backend::Adb(adb) => adb.platform(&self.id),
            Backend::Host(host) => host.platform(),
            Backend::Imd(imd) => imd.platform(&self.id),
        }
    }

    pub fn arch(&self) -> Result<Arch> {
        match &self.backend {
            Backend::Adb(adb) => adb.arch(&self.id),
            Backend::Host(host) => host.arch(),
            Backend::Imd(imd) => imd.arch(&self.id),
        }
    }

    pub fn details(&self) -> Result<String> {
        match &self.backend {
            Backend::Adb(adb) => adb.details(&self.id),
            Backend::Host(host) => host.details(),
            Backend::Imd(imd) => imd.details(&self.id),
        }
    }

    pub fn run(&self, path: &Path, env: &BuildEnv, attach: bool) -> Result<()> {
        let run = match &self.backend {
            Backend::Adb(adb) => adb.run(&self.id, path, env, attach),
            Backend::Host(host) => host.run(path, attach),
            Backend::Imd(imd) => imd.run(&self.id, path, env, attach),
        }?;
        if let Some(url) = run.url {
            std::thread::spawn(run.logger);
            self.attach(&url, env.root_dir(), env.target_file())?;
        } else {
            (run.logger)();
        }
        Ok(())
    }

    pub fn attach(&self, url: &str, root_dir: &Path, target: &Path) -> Result<()> {
        match &self.backend {
            Backend::Adb(adb) => adb.attach(&self.id, url, root_dir, target),
            Backend::Host(host) => host.attach(url, root_dir, target),
            Backend::Imd(imd) => imd.attach(&self.id, url, root_dir, target),
        }
    }

    pub fn xrun_host(&self, path: &Path, attach: bool) -> Result<Run> {
        if let Backend::Host(host) = &self.backend {
            host.run(path, attach)
        } else {
            anyhow::bail!("not host");
        }
    }

    pub fn xrun_adb(
        &self,
        path: &Path,
        package: &str,
        activity: &str,
        attach: bool,
    ) -> Result<Run> {
        if let Backend::Adb(adb) = &self.backend {
            adb.xrun(&self.id, path, package, activity, attach)
        } else {
            anyhow::bail!("not adb");
        }
    }
}

pub struct Run {
    pub url: Option<String>,
    pub logger: Box<dyn FnOnce() + Send>,
    pub child: Option<Child>,
}
