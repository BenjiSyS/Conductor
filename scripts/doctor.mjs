// Checks that this computer can build Conductor and prints exactly what to
// install when something is missing. Run: npm run doctor
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';

const results = [];
const run = (cmd, args = []) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], shell: process.platform === 'win32' }).trim();
  } catch {
    return null;
  }
};
const version = (text) => (text?.match(/(\d+)\.(\d+)(?:\.(\d+))?/) ?? []).slice(1).map((n) => Number(n ?? 0));
const atLeast = (have, want) => {
  for (let i = 0; i < want.length; i++) {
    if ((have[i] ?? 0) !== want[i]) return (have[i] ?? 0) > want[i];
  }
  return true;
};
const check = (name, ok, detail, fix) => results.push({ name, ok, detail, fix });

const platform = process.platform;
let distro = '';
if (platform === 'linux' && fs.existsSync('/etc/os-release')) {
  const rel = fs.readFileSync('/etc/os-release', 'utf8');
  const get = (k) => (rel.match(new RegExp(`^${k}="?([^"\\n]*)`, 'm')) ?? [])[1] ?? '';
  distro = `${get('ID')} ${get('ID_LIKE')}`.toLowerCase();
}
const linuxFamily = /arch|manjaro|endeavour/.test(distro)
  ? 'arch'
  : /fedora|rhel|centos|rocky|alma/.test(distro)
    ? 'fedora'
    : /suse/.test(distro)
      ? 'suse'
      : 'debian';

// Node.js
const node = version(process.versions.node);
check('Node.js 22 or newer', atLeast(node, [22]), `found ${process.versions.node}`, {
  win32: 'winget install OpenJS.NodeJS.LTS',
  darwin: 'brew install node',
  linux: 'Install Node.js 22+ from https://nodejs.org (or with nvm: nvm install 22)',
}[platform]);

// Git
const git = run('git', ['--version']);
check('Git', !!git, git ?? 'not found', { win32: 'winget install Git.Git', darwin: 'xcode-select --install', linux: { debian: 'sudo apt install git', fedora: 'sudo dnf install git', arch: 'sudo pacman -S git', suse: 'sudo zypper in git' }[linuxFamily] }[platform]);

// Rust
const rustc = run('rustc', ['--version']);
const cargo = run('cargo', ['--version']);
check('Rust 1.85 or newer (rustc + cargo)', !!rustc && !!cargo && atLeast(version(rustc), [1, 85]), rustc ?? 'not found', platform === 'win32' ? 'winget install Rustlang.Rustup   (then open a new terminal)' : "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   (then open a new terminal)");

// Platform requirements
if (platform === 'win32') {
  const vswhere = `${process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)'}\\Microsoft Visual Studio\\Installer\\vswhere.exe`;
  const vc = fs.existsSync(vswhere) && run(`"${vswhere}"`, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath']);
  check('Visual Studio C++ build tools', !!vc, vc || 'not found', 'winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"');
  const wv = run('reg', ['query', 'HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', '/v', 'pv'])
    ?? run('reg', ['query', 'HKCU\\Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', '/v', 'pv']);
  check('Microsoft Edge WebView2 runtime', !!wv, wv ? (wv.match(/pv\s+REG_SZ\s+(\S+)/) ?? [])[1] ?? 'present' : 'not found', 'winget install Microsoft.EdgeWebView2Runtime');
} else if (platform === 'darwin') {
  const xc = run('xcode-select', ['-p']);
  check('Xcode command-line tools', !!xc, xc ?? 'not found', 'xcode-select --install');
} else {
  const pkgs = ['webkit2gtk-4.1', 'javascriptcoregtk-4.1', 'libsoup-3.0', 'librsvg-2.0', 'openssl'];
  const pc = run('pkg-config', ['--version']);
  const missing = pc ? pkgs.filter((p) => run('pkg-config', ['--exists', p]) === null) : pkgs;
  const tray = pc && (run('pkg-config', ['--exists', 'ayatana-appindicator3-0.1']) !== null || run('pkg-config', ['--exists', 'appindicator3-0.1']) !== null);
  if (!tray) missing.push('appindicator3');
  const cc = run('cc', ['--version']);
  if (!cc) missing.push('C compiler');
  const install = {
    debian: 'sudo apt update && sudo apt install -y build-essential curl wget file pkg-config libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev',
    fedora: 'sudo dnf install -y webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel libxdo-devel && sudo dnf group install -y c-development',
    arch: 'sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool',
    suse: 'sudo zypper in -y webkit2gtk3-devel libopenssl-devel curl wget file libappindicator3-1 librsvg-devel && sudo zypper in -y -t pattern devel_basis',
  }[linuxFamily];
  check('Linux desktop libraries (WebKitGTK 4.1 and friends)', missing.length === 0, missing.length ? `missing: ${missing.join(', ')}` : 'all present', install);
}

// Project dependencies
check('npm packages installed', fs.existsSync('node_modules/@tauri-apps/cli'), fs.existsSync('node_modules') ? 'node_modules present' : 'not installed', 'npm ci');

let failed = 0;
console.log(`Conductor build check (${os.type()} ${os.release()}${distro ? `, ${distro.trim()}` : ''})\n`);
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.name} — ${r.detail}`);
  if (!r.ok) {
    failed++;
    console.log(`    fix: ${r.fix}`);
  }
}
console.log(failed ? `\n${failed} thing(s) to install, then run npm run doctor again.` : '\nReady. Run: npm run desktop   (or npm run desktop:build for an installer)');
process.exit(failed ? 1 : 0);
