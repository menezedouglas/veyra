#!/usr/bin/env node
import { existsSync } from 'node:fs'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const app = fileURLToPath(new URL('../apps/desktop/', import.meta.url))
const commands = {
  dev: [['vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1420', '--strictPort']],
  build: [['vue-tsc/bin/vue-tsc.js', '--noEmit'], ['vite/bin/vite.js', 'build']],
  typecheck: [['vue-tsc/bin/vue-tsc.js', '--noEmit']],
}
const [mode, ...args] = process.argv.slice(2)
if (!commands[mode]) {
  console.error('Uso: node scripts/frontend.mjs dev|build|typecheck [opções]')
  process.exit(1)
}

for (const [entry, ...options] of commands[mode]) {
  const executable = path.join(app, 'node_modules', entry)
  if (!existsSync(executable)) {
    console.error('Dependências do frontend ausentes. Execute pnpm install --frozen-lockfile.')
    process.exit(1)
  }
  const code = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [executable, ...options, ...args], { cwd: app, stdio: 'inherit' })
    child.on('error', reject)
    child.on('exit', (status) => resolve(status ?? 1))
  }).catch((error) => {
    console.error(error.message)
    return 1
  })
  if (code !== 0) process.exit(code)
}
