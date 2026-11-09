#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs'
import { spawn, spawnSync } from 'node:child_process'
import { homedir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createInterface } from 'node:readline'

const root = fileURLToPath(new URL('../', import.meta.url))
const app = path.join(root, 'apps', 'desktop')

function option(args, names) {
  for (let i = 0; i < args.length && args[i] !== '--'; i++) {
    for (const name of names) {
      if (args[i] === name) {
        if (!args[i + 1] || args[i + 1].startsWith('-')) throw new Error(`Falta valor para ${name}.`)
        return args[i + 1]
      }
      if (args[i].startsWith(`${name}=`)) {
        const value = args[i].slice(name.length + 1)
        if (!value) throw new Error(`Falta valor para ${name}.`)
        return value
      }
      if (name.length === 2 && !name.startsWith('--') && args[i].startsWith(name) && args[i].length > name.length) {
        return args[i].slice(name.length)
      }
    }
  }
}

export function desktopPlan(mode, args = [], platform = process.platform, env = {}) {
  if (!['dev', 'build:windows'].includes(mode)) {
    throw new Error('Uso: node scripts/desktop.mjs dev|build:windows [opções do Tauri]')
  }
  if (mode === 'build:windows' && platform !== 'win32') {
    throw new Error('O instalador Windows deve ser compilado no Windows com a toolchain MSVC.')
  }
  const explicitTarget = option(args, ['--target', '-t'])
  const target = explicitTarget || env.CARGO_BUILD_TARGET
  if (platform === 'win32' && target && !/^(x86_64|aarch64|i686)-pc-windows-msvc$/.test(target)) {
    throw new Error('Use um target Windows MSVC, por exemplo --target x86_64-pc-windows-msvc.')
  }
  if (target && !/^[A-Za-z0-9_-]+$/.test(target)) {
    throw new Error('Use um target triple Rust. Arquivos JSON de target personalizado não são suportados pelo launcher.')
  }
  const cargoSeparator = args.indexOf('--')
  const appSeparator = cargoSeparator < 0 ? -1 : args.indexOf('--', cargoSeparator + 1)
  const buildOptions = args.slice(0, appSeparator < 0 ? args.length : appSeparator)
  if (buildOptions.some((value) => value === '--profile' || value.startsWith('--profile='))) {
    throw new Error('Perfis Cargo personalizados não são suportados. Use dev, dev --release ou build:windows --debug.')
  }
  const cargoOptions = cargoSeparator < 0 ? [] : args.slice(cargoSeparator + 1, appSeparator < 0 ? args.length : appSeparator)
  if (cargoOptions.some((value) => /^(--target|--target-dir|--release|--config)(=|$)/.test(value) || value === '-r')) {
    throw new Error('Passe --target e --release ao Tauri antes de --. Use CARGO_TARGET_DIR para alterar a pasta de build.')
  }
  const tauriOptions = args.slice(0, args.indexOf('--') < 0 ? args.length : args.indexOf('--'))
  const debug = mode === 'dev' ? !tauriOptions.includes('--release') : tauriOptions.includes('--debug') || tauriOptions.includes('-d')
  const sidecars = ['scripts/prepare-sidecars.py']
  if (debug) sidecars.push('--debug')
  if (target) sidecars.push('--target', target)
  if (tauriOptions.some((value) => value.split(/[=,]/).includes('native-libnice'))) sidecars.push('--native-libnice')
  const inheritedTarget = !explicitTarget && target ? ['--target', target] : []
  const tauri = mode === 'dev'
    ? ['dev', ...inheritedTarget, ...args]
    : ['build', '--config', 'src-tauri/tauri.release.conf.json', '--bundles', 'nsis', ...inheritedTarget, ...args]
  return { sidecars, tauri }
}

export function windowsToolchain(channel, arch = process.arch) {
  const host = arch === 'arm64' ? 'aarch64' : 'x86_64'
  return `${channel}-${host}-pc-windows-msvc`
}

export function needsLocalOperator(mode, env) {
  return mode === 'dev' && env.VEYRA_DEV_LOCAL_OPERATOR !== '0'
    && !Object.hasOwn(env, 'VEYRA_SIGNALING_ENDPOINT')
    && !Object.hasOwn(env, 'VEYRA_DEFAULT_SIGNALING_ENDPOINT')
}

function executable(name, env) {
  const pathKey = Object.keys(env).find((key) => key.toLowerCase() === 'path') ?? 'PATH'
  const extensions = process.platform === 'win32' ? ['.exe', '.cmd', '.bat', ''] : ['']
  for (const directory of (env[pathKey] ?? '').split(path.delimiter)) {
    for (const extension of extensions) {
      const candidate = path.join(directory, name + extension)
      if (existsSync(candidate)) return candidate
    }
  }
}

export function desktopEnvironment(input = process.env, cwd = process.cwd()) {
  const env = { ...input }
  const pathKey = Object.keys(env).find((key) => key.toLowerCase() === 'path') ?? 'PATH'
  const cargoBin = path.join(env.CARGO_HOME || path.join(homedir(), '.cargo'), 'bin')
  env[pathKey] = [cargoBin, env[pathKey]].filter(Boolean).join(path.delimiter)
  // Cargo resolves relative target directories from each subprocess's cwd.
  // Both the sidecar build and Tauri must use the caller's same directory.
  if (env.CARGO_TARGET_DIR) env.CARGO_TARGET_DIR = path.resolve(cwd, env.CARGO_TARGET_DIR)
  return env
}

function preflightRust(env) {
  const rustup = executable('rustup', env)
  if (!rustup) throw new Error('Rust não encontrado. Instale Rust com rustup e abra um novo terminal: https://rustup.rs/')
  const channel = readFileSync(path.join(root, 'rust-toolchain.toml'), 'utf8').match(/^channel\s*=\s*"([^"]+)"/m)?.[1]
  if (!channel) throw new Error('Canal Rust ausente em rust-toolchain.toml.')
  if (process.platform === 'win32') {
    const toolchain = windowsToolchain(channel)
    env.RUSTUP_TOOLCHAIN = toolchain
    const installed = spawnSync(rustup, ['toolchain', 'list'], { env, encoding: 'utf8', windowsHide: true })
    if (installed.status !== 0 || !installed.stdout.split(/\r?\n/).some((line) => line.split(' ')[0] === toolchain)) {
      throw new Error(`Toolchain MSVC ausente. Execute:\n  rustup toolchain install ${toolchain} --profile minimal --component rustfmt --component clippy\nO desktop Tauri no Windows usa MSVC; a toolchain GNU não é suficiente.`)
    }
    const vswhere = path.join(env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'Microsoft Visual Studio', 'Installer', 'vswhere.exe')
    const visualStudio = existsSync(vswhere) && spawnSync(vswhere, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], { encoding: 'utf8', windowsHide: true }).stdout?.trim()
    if (!visualStudio && !executable('cl', env)) {
      throw new Error('Build Tools C++ da Microsoft ausentes. Instale Visual Studio Build Tools com "Desenvolvimento para desktop com C++" e Windows SDK, e abra um novo terminal: https://visualstudio.microsoft.com/visual-cpp-build-tools/')
    }
  }
}

function python(env) {
  for (const [name, options] of [['python', []], ['python3', []], ['py', ['-3']]]) {
    const command = executable(name, env)
    if (command && spawnSync(command, [...options, '-c', 'import sys; sys.exit(0 if sys.version_info[0] == 3 else 1)'], { env, stdio: 'ignore', windowsHide: true }).status === 0) return [command, options]
  }
  throw new Error('Python 3 não encontrado. Instale-o para preparar os binários internos do desktop.')
}

function run(command, args, cwd, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env, stdio: 'inherit' })
    child.on('error', reject)
    child.on('exit', (code) => resolve(code ?? 1))
  })
}

async function localOperator(command, prefix, plan, env) {
  const metadata = spawnSync(executable('cargo', env), ['metadata', '--locked', '--no-deps', '--format-version', '1'], { cwd: root, env, encoding: 'utf8', windowsHide: true })
  if (metadata.status !== 0) throw new Error('Não foi possível localizar o serviço de desenvolvimento.')
  const targetDirectory = JSON.parse(metadata.stdout).target_directory
  const target = option(plan.sidecars, ['--target'])
  const directory = target ? path.join(targetDirectory, target) : targetDirectory
  const profile = plan.sidecars.includes('--debug') ? 'debug' : 'release'
  const binary = path.join(directory, profile, process.platform === 'win32' ? 'veyra-signaling.exe' : 'veyra-signaling')
  const helper = spawn(command, [...prefix, '-u', path.join(root, 'scripts', 'local-operator.py'), '--binary', binary, '--serve'], {
    cwd: root, env, stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true,
  })
  helper.stdin.on('error', () => {})
  const ended = new Promise(resolve => { helper.once('exit', resolve); helper.once('error', resolve) })
  const stop = async () => {
    if (helper.exitCode !== null) return
    helper.stdin.end('stop\n')
    const force = setTimeout(() => helper.kill(), 15000)
    try { await ended } finally { clearTimeout(force) }
  }
  const lines = createInterface({ input: helper.stdout })
  let timer
  let setup
  try {
    setup = await new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(new Error('O serviço local de desenvolvimento não iniciou a tempo.')), 45000)
      helper.once('error', () => reject(new Error('Não foi possível iniciar o serviço local de desenvolvimento.')))
      helper.once('exit', () => reject(new Error('O serviço local de desenvolvimento foi encerrado antes de iniciar.')))
      lines.once('line', line => {
        try {
          if (line.length > 8192) throw new Error()
          const value = JSON.parse(line)
          const endpoint = new URL(value.endpoint)
          if (value.scope !== 'local-development' || endpoint.protocol !== 'wss:' || endpoint.hostname !== '127.0.0.1'
            || !endpoint.port || endpoint.pathname !== '/v1/rendezvous' || endpoint.username || endpoint.password
            || endpoint.search || endpoint.hash || !path.isAbsolute(value.caFile) || !existsSync(value.caFile)) throw new Error()
          resolve(value)
        } catch { reject(new Error('O serviço local retornou uma configuração inválida.')) }
      })
    })
  } catch (error) {
    await stop()
    throw error
  } finally {
    clearTimeout(timer)
    lines.close()
  }
  return {
    ...setup,
    stop,
  }
}

async function main() {
  const [mode, ...args] = process.argv.slice(2)
  if (!mode || mode === '--help' || mode === '-h') {
    console.log('Uso: node scripts/desktop.mjs dev|build:windows [opções do Tauri]')
    return
  }
  const env = desktopEnvironment()
  const cli = path.join(app, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
  if (!existsSync(cli)) throw new Error('CLI Tauri ausente. Execute pnpm install --frozen-lockfile.')
  if (['dev', 'build:windows'].includes(mode) && (args.includes('--help') || args.includes('-h'))) {
    return run(process.execPath, [cli, mode === 'dev' ? 'dev' : 'build', ...args], app, env)
  }
  const plan = desktopPlan(mode, args, process.platform, env)
  preflightRust(env)
  const [command, prefix] = python(env)
  const startLocal = needsLocalOperator(mode, env)
  const code = await run(command, [...prefix, ...plan.sidecars, ...(startLocal ? ['--with-signaling'] : [])], root, env)
  if (code !== 0) return code
  let operator
  // Child processes receive Ctrl-C too; keep the supervisor alive for cleanup.
  const interrupt = () => {}
  process.on('SIGINT', interrupt)
  try {
    if (startLocal) {
      operator = await localOperator(command, prefix, plan, env)
      env.VEYRA_SIGNALING_ENDPOINT = operator.endpoint
      env.SSL_CERT_FILE = operator.caFile
      env.VEYRA_DEV_LOCAL_OPERATOR_ACTIVE = '1'
      console.log('Serviço de desenvolvimento local iniciado. Os convites deste modo funcionam nesta máquina; acesso pela internet usa o serviço da distribuição.')
    }
    return await run(process.execPath, [cli, ...plan.tauri], app, env)
  } finally {
    if (operator) await operator.stop()
    process.off('SIGINT', interrupt)
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = (await main()) ?? 0
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
