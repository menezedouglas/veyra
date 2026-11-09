import { expect, it, vi } from 'vitest'
const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => false, invoke }))
import { api, DesktopRequiredError } from './ipc'

it('recusa comandos sensíveis no navegador sem simular sessão', async () => {
  await expect(api.startSession()).rejects.toBeInstanceOf(DesktopRequiredError)
  await expect(api.openOnce('some-id')).rejects.toBeInstanceOf(DesktopRequiredError)
  await expect(api.sendMessage(['peer'], 'teste sintético')).rejects.toBeInstanceOf(DesktopRequiredError)
  expect(invoke).not.toHaveBeenCalled()
})
