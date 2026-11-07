// Build the locked native worker for this host and place it under the runtime platform name.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..'),
  native = path.join(root, 'native');
if (process.env.CARGO_BUILD_TARGET)
  throw new Error(
    'Build cross-target workers explicitly with Cargo and name their platform asset separately.',
  );
if (process.env.RUSTFLAGS && !process.env.CARGO_ENCODED_RUSTFLAGS)
  throw new Error(
    'Use CARGO_ENCODED_RUSTFLAGS for custom compiler flags so paths containing spaces remain intact.',
  );
const flags = (process.env.CARGO_ENCODED_RUSTFLAGS || '').split('\x1f').filter(Boolean);
const windowsTargets = {
  x64: 'x86_64-pc-windows-msvc',
  arm64: 'aarch64-pc-windows-msvc',
  ia32: 'i686-pc-windows-msvc',
};
const targetTriple = process.platform === 'win32' ? windowsTargets[process.arch] : undefined;
if (process.platform === 'win32' && !targetTriple)
  throw new Error('Unsupported Windows architecture');
if (targetTriple && !flags.includes('-Ctarget-feature=+crt-static'))
  flags.push('-Ctarget-feature=+crt-static');
for (const flag of [
  `--remap-path-prefix=${root}=/n2-kraken`,
  `--remap-path-prefix=${os.homedir()}=/build-home`,
])
  if (!flags.includes(flag)) flags.push(flag);
const result = spawnSync(
  'cargo',
  [
    '+1.98.0',
    'build',
    '--manifest-path',
    'Cargo.toml',
    '--release',
    '--locked',
    ...(targetTriple ? ['--target', targetTriple] : []),
    '--bin',
    'n2-kraken-worker',
    '-j',
    '2',
  ],
  {
    cwd: native,
    stdio: 'inherit',
    windowsHide: true,
    env: { ...process.env, CARGO_ENCODED_RUSTFLAGS: flags.join('\x1f') },
  },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const extension = process.platform === 'win32' ? '.exe' : '';
const target = process.env.CARGO_TARGET_DIR
  ? path.resolve(native, process.env.CARGO_TARGET_DIR)
  : path.join(native, 'target');
const destination = path.join(
  root,
  'bin',
  `n2-kraken-worker-${process.platform}-${process.arch}${extension}`,
);
fs.mkdirSync(path.dirname(destination), { recursive: true });
const releaseDirectory = targetTriple
  ? path.join(target, targetTriple, 'release')
  : path.join(target, 'release');
fs.copyFileSync(path.join(releaseDirectory, `n2-kraken-worker${extension}`), destination);
fs.chmodSync(destination, 0o755);
process.stdout.write(`Native worker built for ${process.platform}-${process.arch}\n`);
