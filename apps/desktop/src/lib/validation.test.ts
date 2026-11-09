import { describe, expect, it } from 'vitest'
import { validateText, formatRemaining, validateSignalingEndpoint } from './validation'

describe('limites do compositor', () => {
  it('limita bytes UTF-8 e aceita limite exato', () => {
    expect(validateText('á'.repeat(2048))).toBeUndefined()
    expect(validateText('á'.repeat(2049))).toContain('4 KiB')
  })
  it('rejeita texto vazio e controles, sem impedir nova linha', () => {
    expect(validateText('   \n')).toBeDefined()
    expect(validateText('texto\u0000')).toBeDefined()
    expect(validateText('duas\nlinhas')).toBeUndefined()
  })
  it('nunca exibe prazo negativo', () => {
    expect(formatRemaining(-10)).toBe('0:00')
    expect(formatRemaining(300000)).toBe('5:00')
    expect(formatRemaining(1)).toBe('0:01')
  })
})


describe('endpoint de sinalização', () => {
  it('permite somente WSS no caminho fechado e sem tokens ou credenciais', () => {
    expect(validateSignalingEndpoint('')).toBeUndefined()
    expect(validateSignalingEndpoint('wss://operator.invalid/v1/rendezvous')).toBeUndefined()
    for (const endpoint of ['http://operator.invalid/v1/rendezvous', 'wss://operator.invalid/other', 'wss://user:password@operator.invalid/v1/rendezvous', 'wss://operator.invalid/v1/rendezvous?token=synthetic', 'wss://operator.invalid/v1/rendezvous#synthetic']) expect(validateSignalingEndpoint(endpoint)).toBeDefined()
  })
})
