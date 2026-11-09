export const MAX_TEXT_BYTES = 4096

// UX apenas: o núcleo Rust deve validar novamente toda entrada.
export function validateText(text: string): string | undefined {
  if (text.trim().length === 0) return 'Digite uma mensagem.'
  if (new TextEncoder().encode(text).length > MAX_TEXT_BYTES) return 'A mensagem deve ter até 4 KiB em UTF-8.'
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(text)) return 'Remova os caracteres de controle da mensagem.'
  return undefined
}

export function formatRemaining(milliseconds: number): string {
  const seconds = Math.max(0, Math.ceil(milliseconds / 1000))
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
}

export function validateSignalingEndpoint(value: string): string | undefined {
  if (!value.trim()) return undefined
  try {
    const url = new URL(value.trim())
    if (url.protocol !== 'wss:' || !url.hostname || url.username || url.password || url.search || url.hash || url.pathname !== '/v1/rendezvous') return 'Use o endereço WSS do operador, com caminho /v1/rendezvous e sem credenciais, parâmetros ou fragmentos.'
  } catch { return 'Informe um endereço WSS válido do operador.' }
  return undefined
}
