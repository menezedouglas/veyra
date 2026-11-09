import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
const css = readFileSync(resolve(process.cwd(), 'src/styles/tokens.css'), 'utf8')

function luminance(hex: string): number {
  const rgb = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255)
  const linear = rgb.map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4)
  return linear[0]! * .2126 + linear[1]! * .7152 + linear[2]! * .0722
}
function contrast(a: string, b: string): number {
  const [min, max] = [luminance(a), luminance(b)].sort((x, y) => x - y)
  return (max! + .05) / (min! + .05)
}

describe('contraste WCAG AA dos tokens sólidos', () => {
  const blocks = css.split(':root').slice(1, 3)
  it('define explicitamente os dois temas', () => { expect(blocks).toHaveLength(2) })
  for (const [index, block] of blocks.entries()) {
    const values = Object.fromEntries([...block.split('@theme')[0]!.matchAll(/--([\w-]+): (#[a-fA-F0-9]{6})/g)].map(m => [m[1], m[2]]))
    for (const [fg, bg] of [['foreground', 'background'], ['muted-foreground', 'card'], ['primary-foreground', 'primary'], ['warning', 'card'], ['destructive', 'card'], ['success', 'card'], ['route-direct', 'card'], ['route-relay', 'card']]) {
      it(`${index === 0 ? 'escuro' : 'claro'} ${fg} sobre ${bg}`, () => {
        expect(contrast(values[fg!]!, values[bg!]!)).toBeGreaterThanOrEqual(4.5)
      })
    }
  }
})
