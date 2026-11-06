import { describe, expect, it } from 'vitest'
import { OUTPUT_HEADER, outputRequest } from './output'

const rect = { x: 10, y: 20, width: 30, height: 40 }
const header = (r: ReturnType<typeof outputRequest>) => JSON.parse(r.headers[OUTPUT_HEADER])

describe('outputRequest', () => {
  it('sends the session and rect alone without annotations', () => {
    for (const out of [
      undefined,
      null,
      { rgba: null, x: 0, y: 0, width: 0, height: 0, blurs: [] },
    ]) {
      const r = outputRequest(3, rect, out)
      expect(r.body).toHaveLength(0)
      expect(header(r)).toEqual({ session: 3, rect, annotations: null })
    }
  })

  it('sends the layer pixels raw and where they go in the header', () => {
    const rgba = new Uint8ClampedArray([1, 2, 3, 4, 5, 6, 7, 8])
    const blurs = [{ x: 0, y: 0, width: 5, height: 5 }]
    const r = outputRequest(3, rect, { rgba, x: 4, y: 5, width: 2, height: 1, blurs })
    expect(r.body).toBeInstanceOf(Uint8Array)
    expect([...r.body]).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
    // The same memory, not a copy.
    expect(r.body.buffer).toBe(rgba.buffer)
    expect(header(r)).toEqual({
      session: 3,
      rect,
      annotations: { x: 4, y: 5, width: 2, height: 1, blurs },
    })
  })

  it('sends blurs with no layer', () => {
    const blurs = [{ x: 0, y: 0, width: 5, height: 5 }]
    const r = outputRequest(3, rect, { rgba: null, x: 0, y: 0, width: 0, height: 0, blurs })
    expect(r.body).toHaveLength(0)
    expect(header(r).annotations).toEqual({ x: 0, y: 0, width: 0, height: 0, blurs })
  })
})
