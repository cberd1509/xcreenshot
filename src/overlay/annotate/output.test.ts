import { describe, expect, it } from 'vitest'
import { OUTPUT_HEADER, outputRequest } from './output'

const rect = { x: 10, y: 20, width: 30, height: 40 }
const header = (r: ReturnType<typeof outputRequest>) => JSON.parse(r.headers[OUTPUT_HEADER])
const blurs = [{ x: 0, y: 0, width: 5, height: 5 }]

describe('outputRequest', () => {
  it('sends the session and rect alone without annotations', () => {
    for (const out of [undefined, null, { layer: null, blurs: [] }]) {
      const r = outputRequest(3, rect, out)
      expect(r.body).toHaveLength(0)
      expect(header(r)).toEqual({ session: 3, rect, blurs: [] })
    }
  })

  it('sends the layer pixels raw and where they go in the header', () => {
    const rgba = new Uint8ClampedArray([1, 2, 3, 4, 5, 6, 7, 8])
    const r = outputRequest(3, rect, { layer: { rgba, x: 4, y: 5, width: 2, height: 1 }, blurs })
    expect(r.body).toBeInstanceOf(Uint8Array)
    expect([...r.body]).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
    // The same memory, not a copy.
    expect(r.body.buffer).toBe(rgba.buffer)
    expect(header(r)).toEqual({
      session: 3,
      rect,
      layer: { x: 4, y: 5, width: 2, height: 1 },
      blurs,
    })
  })

  it('sends blurs with no layer', () => {
    const r = outputRequest(3, rect, { layer: null, blurs })
    expect(r.body).toHaveLength(0)
    expect(header(r)).toEqual({ session: 3, rect, blurs })
  })
})
