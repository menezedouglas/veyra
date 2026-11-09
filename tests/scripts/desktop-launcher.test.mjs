import test from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { desktopPlan, desktopEnvironment, windowsToolchain, needsLocalOperator, pythonCandidates } from '../../scripts/desktop.mjs'

test('direct development starts no server; a local operator requires explicit opt-in', () => {
  assert.equal(needsLocalOperator('dev', {}), false)
  assert.equal(needsLocalOperator('dev', { VEYRA_DEV_LOCAL_OPERATOR: '1' }), true)
  assert.equal(needsLocalOperator('dev', { VEYRA_DEV_LOCAL_OPERATOR: '1', VEYRA_SIGNALING_ENDPOINT: '' }), false)
  assert.equal(needsLocalOperator('build:windows', {}), false)
  assert.equal(needsLocalOperator('dev', { VEYRA_SIGNALING_ENDPOINT: '' }), false)
  assert.equal(needsLocalOperator('dev', { VEYRA_SIGNALING_ENDPOINT: 'invalid' }), false)
  assert.equal(needsLocalOperator('dev', { VEYRA_DEFAULT_SIGNALING_ENDPOINT: 'wss://synthetic.invalid/v1/rendezvous' }), false)
  assert.equal(needsLocalOperator('dev', { VEYRA_DEV_LOCAL_OPERATOR: '0' }), false)
})

test('development prepares debug sidecars for the same target and preserves Tauri options', () => {
  const args = ['--target', 'x86_64-pc-windows-msvc', '--config', 'src-tauri/local.json', '--no-watch']
  const plan = desktopPlan('dev', args, 'win32')
  assert.deepEqual(plan.sidecars, ['scripts/prepare-sidecars.py', '--debug', '--target', 'x86_64-pc-windows-msvc'])
  assert.deepEqual(plan.tauri, ['dev', ...args])
})

test('Windows installer includes the sidecars configuration before user overrides', () => {
  const args = ['--target=aarch64-pc-windows-msvc', '--config', 'src-tauri/local.json']
  const plan = desktopPlan('build:windows', args, 'win32')
  assert.deepEqual(plan.sidecars, ['scripts/prepare-sidecars.py', '--target', 'aarch64-pc-windows-msvc'])
  assert.deepEqual(plan.tauri, ['build', '--config', 'src-tauri/tauri.release.conf.json', '--bundles', 'nsis', ...args])
})

test('attached short target options stage ARM sidecars instead of host binaries', () => {
  for (const target of ['-taarch64-pc-windows-msvc', '-t=aarch64-pc-windows-msvc']) {
    const plan = desktopPlan('build:windows', [target], 'win32')
    assert.deepEqual(plan.sidecars, ['scripts/prepare-sidecars.py', '--target', 'aarch64-pc-windows-msvc'])
    assert.equal(plan.tauri.at(-1), target)
  }
})

test('release development and debug installers stage matching profiles', () => {
  assert.deepEqual(desktopPlan('dev', ['--release'], 'win32').sidecars, ['scripts/prepare-sidecars.py'])
  assert.deepEqual(desktopPlan('build:windows', ['--debug'], 'win32').sidecars, ['scripts/prepare-sidecars.py', '--debug'])
})

test('rejects unsupported Windows targets and missing target values before building', () => {
  assert.throws(() => desktopPlan('dev', ['--target', 'x86_64-pc-windows-gnu'], 'win32'), /MSVC/)
  assert.throws(() => desktopPlan('build:windows', [], 'linux'), /Windows/)
  assert.throws(() => desktopPlan('dev', ['--target'], 'win32'), /Falta valor/)
  assert.throws(() => desktopPlan('dev', ['--target='], 'win32'), /Falta valor/)
})

test('application arguments do not alter the sidecar target or build profile', () => {
  assert.deepEqual(desktopPlan('dev', ['--', '--', '--target', 'example', '--release', '--profile', 'app-value'], 'win32').sidecars, ['scripts/prepare-sidecars.py', '--debug'])
})

test('MSVC host matches the pinned channel and machine architecture', () => {
  assert.equal(windowsToolchain('1.99.1', 'x64'), '1.99.1-x86_64-pc-windows-msvc')
  assert.equal(windowsToolchain('1.99.1', 'arm64'), '1.99.1-aarch64-pc-windows-msvc')
})

test('Python discovery reaches installed launchers and runtimes after an unusable WindowsApps alias', () => {
  const env = { Path: 'C:\\Users\\Fixture\\WindowsApps', LOCALAPPDATA: 'C:\\Users\\Fixture User\\AppData\\Local', WINDIR: 'C:\\Windows' }
  const original = { ...env }
  const candidates = pythonCandidates(env, ['Python312-32', 'Launcher', 'Python314', '..', 'Python314/escape', 'Python../escape'], 'win32')
  assert.deepEqual(candidates[0], ['C:\\Users\\Fixture\\WindowsApps\\python.exe', []])
  assert.deepEqual(candidates.find(([command]) => command.endsWith('Launcher\\py.exe')), ['C:\\Users\\Fixture User\\AppData\\Local\\Programs\\Python\\Launcher\\py.exe', ['-3']])
  const runtimes = candidates.filter(([command]) => /Python\d[^\\]*\\python\.exe$/.test(command))
  assert.deepEqual(runtimes, [
    ['C:\\Users\\Fixture User\\AppData\\Local\\Programs\\Python\\Python314\\python.exe', []],
    ['C:\\Users\\Fixture User\\AppData\\Local\\Programs\\Python\\Python312-32\\python.exe', []],
  ])
  assert.deepEqual(candidates.at(-1), ['C:\\Windows\\py.exe', ['-3']])
  assert.deepEqual(env, original)
})

test('Python candidates check every PATH location and avoid duplicate paths or Windows discovery on Linux', () => {
  assert.deepEqual(pythonCandidates({ PATH: '/usr/local/bin:/usr/bin:/usr/bin', LOCALAPPDATA: '/ignored' }, ['Python314'], 'linux'), [
    ['/usr/local/bin/python', []], ['/usr/bin/python', []],
    ['/usr/local/bin/python3', []], ['/usr/bin/python3', []],
    ['/usr/local/bin/py', ['-3']], ['/usr/bin/py', ['-3']],
  ])
  const windows = pythonCandidates({ Path: '"C:\\Python314";c:\\Python314', SystemRoot: 'D:\\Windows' }, [], 'win32')
  assert.equal(windows.filter(([command]) => command.toLowerCase() === 'c:\\python314\\python.exe').length, 1)
  assert.deepEqual(windows.at(-1), ['D:\\Windows\\py.exe', ['-3']])
})

test('Cargo environment target is shared with sidecars and Tauri, and explicit flags override it', () => {
  const env = { CARGO_BUILD_TARGET: 'aarch64-pc-windows-msvc' }
  const inherited = desktopPlan('dev', [], 'win32', env)
  assert.ok(inherited.sidecars.includes('aarch64-pc-windows-msvc'))
  assert.deepEqual(inherited.tauri, ['dev', '--target', 'aarch64-pc-windows-msvc'])
  const explicit = desktopPlan('dev', ['--target', 'x86_64-pc-windows-msvc'], 'win32', env)
  assert.ok(explicit.sidecars.includes('x86_64-pc-windows-msvc'))
  assert.ok(!explicit.tauri.includes('aarch64-pc-windows-msvc'))
  assert.throws(() => desktopPlan('dev', [], 'win32', { CARGO_BUILD_TARGET: 'x86_64-pc-windows-gnu' }), /MSVC/)
})

test('custom Cargo profiles fail clearly instead of bundling mismatched sidecars', () => {
  assert.throws(() => desktopPlan('build:windows', ['--', '--profile', 'fast'], 'win32'), /Perfis Cargo personalizados/)
  assert.throws(() => desktopPlan('dev', ['--', '--target', 'x86_64-pc-windows-msvc'], 'win32'), /antes de --/)
})

test('relative Cargo target directories resolve once before subprocess working directories change', () => {
  const caller = path.join(tmpdir(), 'veyra-launcher')
  const original = { CARGO_TARGET_DIR: path.join('build', 'native') }
  const env = desktopEnvironment(original, caller)
  assert.equal(env.CARGO_TARGET_DIR, path.join(caller, 'build', 'native'))
  assert.equal(original.CARGO_TARGET_DIR, path.join('build', 'native'))
  assert.equal(desktopEnvironment(env, path.join(caller, 'apps', 'desktop')).CARGO_TARGET_DIR, env.CARGO_TARGET_DIR)
})

test('CLI help works before Rust, Python and C++ preflight', () => {
  const entry = fileURLToPath(new URL('../../scripts/desktop.mjs', import.meta.url))
  for (const flag of ['-h', '--help']) {
    const env = { ...process.env, PATH: '', Path: '', CARGO_HOME: path.join(tmpdir(), `veyra-no-cargo-${process.pid}`), CARGO_BUILD_TARGET: 'unavailable' }
    const result = spawnSync(process.execPath, [entry, 'dev', flag], { encoding: 'utf8', env })
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /Run your app in development mode/)
  }
})

test('invalid launcher modes fail before starting build processes', () => {
  for (const script of ['desktop.mjs', 'frontend.mjs']) {
    const entry = fileURLToPath(new URL(`../../scripts/${script}`, import.meta.url))
    const result = spawnSync(process.execPath, [entry, 'unknown'], { encoding: 'utf8' })
    assert.equal(result.status, 1)
    assert.match(result.stderr, /Uso: node scripts\//)
  }
})
