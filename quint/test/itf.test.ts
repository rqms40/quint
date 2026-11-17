import { describe, it } from 'mocha'
import { assert } from 'chai'
import { quintExAreEqual } from './util'
import { zip } from '../src/util'

import { buildExpression } from './builders/ir'
import { parseJsonBig } from '../src/jsonHelper'
import { ItfTrace, ofItf, ofItfNormalized, toItf } from '../src/itf'

describe('toItf', () => {
  it('converts two states', () => {
    const trace = ['{ x: 2, y: true }', '{ x: 3, y: false }'].map(buildExpression)

    const itfTrace = toItf(['x', 'y'], trace)
    const expected: ItfTrace = {
      vars: ['x', 'y'],
      states: [
        {
          '#meta': { index: 0 },
          x: { '#bigint': '2' },
          y: true,
        },
        {
          '#meta': { index: 1 },
          x: { '#bigint': '3' },
          y: false,
        },
      ],
    }
    assert(itfTrace.isRight(), `invalid ITF trace: ${JSON.stringify(itfTrace.unwrap)}`)
    assert.deepEqual(itfTrace.unwrap(), expected)

    const roundTripTrace = ofItf(itfTrace.unwrap())
    assert(
      zip(roundTripTrace, trace).every(([a, b]) => quintExAreEqual(a, b)),
      `round trip conversion of trace failed`
    )
  })

  it('converts all kinds', () => {
    const text = `{
  a: 2,
  b: "hello",
  c: 1000000000000000000,
  d: Set(5, 6),
  e: { foo: 3, bar: true },
  f: (7, "myStr"),
  g: Map(1 -> "a", 2 -> "b", 3 -> "c"),
  h: Map(),
  i: Map(1 -> "a"),
  j: variant("A", 2)
}
`
    const trace = [buildExpression(text)]
    const vars = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i']
    const itfTrace = toItf(vars, trace)
    const expected = {
      vars: vars,
      states: [
        {
          '#meta': {
            index: 0,
          },
          a: { '#bigint': '2' },
          b: 'hello',
          c: { '#bigint': '1000000000000000000' },
          d: { '#set': [{ '#bigint': '5' }, { '#bigint': '6' }] },
          e: { foo: { '#bigint': '3' }, bar: true },
          f: { '#tup': [{ '#bigint': '7' }, 'myStr'] },
          g: {
            '#map': [
              [{ '#bigint': '1' }, 'a'],
              [{ '#bigint': '2' }, 'b'],
              [{ '#bigint': '3' }, 'c'],
            ],
          },
          h: { '#map': [] },
          i: { '#map': [[{ '#bigint': '1' }, 'a']] },
          j: { tag: 'A', value: { '#bigint': '2' } },
        },
      ],
    }
    assert(itfTrace.isRight(), `invalid ITF trace: ${JSON.stringify(itfTrace.unwrap)}`)
    assert.deepEqual(itfTrace.unwrap(), expected)

    const roundTripTrace = ofItf(itfTrace.unwrap())
    assert(
      zip(roundTripTrace, trace).every(([a, b]) => quintExAreEqual(a, b)),
      `round trip conversion of trace failed`
    )
  })

  it('converts unit type from Apalache', () => {
    const text = '{ a: () }'

    const trace = [buildExpression(text)]
    const vars = ['a']
    const itfTrace = {
      vars: vars,
      states: [
        {
          '#meta': {
            index: 0,
          },
          a: { tag: 'UNIT' },
        },
      ],
    }

    const roundTripTrace = ofItf(itfTrace)
    assert(
      zip(roundTripTrace, trace).every(([a, b]) => quintExAreEqual(a, b)),
      `round trip conversion of trace failed`
    )
  })

  it('does not confuse records with a tag field for variants', () => {
    const text = `{
  ledger: [{ tag: "pay", amount: 100, dir: variant("D", { from_p: "A", to_p: "B" }) }]
}`

    const trace = [buildExpression(text)]
    const itfTrace = toItf(['ledger'], trace)

    assert(itfTrace.isRight(), `invalid ITF trace: ${JSON.stringify(itfTrace.unwrap)}`)

    const roundTripTrace = ofItf(itfTrace.unwrap())
    assert(
      zip(roundTripTrace, trace).every(([a, b]) => quintExAreEqual(a, b)),
      `round trip conversion of trace failed`
    )
  })
})

describe('ofItf integers parsed by json-bigint', () => {
  // json-bigint turns a JSON number into a bignumber.js value when the token
  // is longer than 15 characters (the minus sign counts). Those must come back
  // as integers, not as {s, e, c} records.
  const cases: [string, string][] = [
    ['10^15 - 1', '999999999999999'],
    ['10^15', '1000000000000000'],
    ['2^53 - 1', '9007199254740991'],
    ['2^63', '9223372036854775808'],
    ['-(10^15 - 1)', '-999999999999999'],
    ['-10^15', '-1000000000000000'],
    ['-10^14', '-100000000000000'],
    ['-(10^14 - 1)', '-99999999999999'],
    ['-(2^53 - 1)', '-9007199254740991'],
  ]

  cases.forEach(([label, digits]) => {
    it(`decodes ${label} as #bigint ${digits}`, () => {
      const trace = parseJsonBig(`{"vars":["x"],"states":[{"x":${digits}}]}`)
      const itf = toItf(['x'], ofItf(trace))
      assert(itf.isRight(), `invalid ITF trace for ${digits}: ${itf.value}`)
      assert.deepEqual(itf.unwrap().states[0].x, { '#bigint': digits })
    })
  })

  // bignumber.js toString() uses exponent notation from 1e21 up.
  ;['1000000000000000000000', '-1000000000000000000000'].forEach(digits => {
    it(`decodes ${digits} in decimal, not exponent notation`, () => {
      const trace = parseJsonBig(`{"vars":["x"],"states":[{"x":${digits}}]}`)
      const itf = toItf(['x'], ofItf(trace))
      assert(itf.isRight(), `invalid ITF trace for ${digits}: ${itf.value}`)
      assert.deepEqual(itf.unwrap().states[0].x, { '#bigint': digits })
    })
  })

  it('does not treat a record containing _isBigNumber as a bignumber', () => {
    const trace = parseJsonBig('{"vars":["r"],"states":[{"r":{"_isBigNumber":true,"x":1}}]}')
    const itf = toItf(['r'], ofItf(trace))
    assert(itf.isRight(), `invalid ITF trace: ${itf.value}`)
    assert.deepEqual(itf.unwrap().states[0].r, { _isBigNumber: true, x: { '#bigint': '1' } })
  })

  it('does not treat a state variable named _isBigNumber as a bignumber', () => {
    const trace = parseJsonBig('{"vars":["_isBigNumber","x"],"states":[{"_isBigNumber":true,"x":1}]}')
    const itf = toItf(['_isBigNumber', 'x'], ofItf(trace))
    assert(itf.isRight(), `invalid ITF trace: ${itf.value}`)
    assert.deepEqual(itf.unwrap().states[0], {
      '#meta': { index: 0 },
      _isBigNumber: true,
      x: { '#bigint': '1' },
    })
  })

  // Built with the constructor of a value json-bigint just produced, including
  // one at the 1e21 toFixed() boundary. This stays an integer even when a
  // direct bignumber.js import is a different copy.
  it('decodes an instance of the constructor json-bigint produced', () => {
    const revived = parseJsonBig('1000000000000000')
    const trace = parseJsonBig('{"vars":["x"],"states":[{}]}')
    trace.states[0].x = new revived.constructor('-1000000000000000000000')
    const itf = toItf(['x'], ofItf(trace))
    assert(itf.isRight(), `invalid ITF trace: ${itf.value}`)
    assert.deepEqual(itf.unwrap().states[0].x, { '#bigint': '-1000000000000000000000' })
  })

  it('does not decode a lookalike from a different constructor', () => {
    const lookalike = Object.assign(Object.create({ toFixed: () => '1000000000000000' }), {
      s: 1,
      e: 15,
      c: [10],
      _isBigNumber: true,
    })
    const trace = parseJsonBig('{"vars":["x"],"states":[{}]}')
    trace.states[0].x = lookalike
    const itf = toItf(['x'], ofItf(trace))
    assert(itf.isRight(), `invalid ITF trace: ${itf.value}`)
    assert.deepEqual(itf.unwrap().states[0].x, {
      s: { '#bigint': '1' },
      e: { '#bigint': '15' },
      c: [{ '#bigint': '10' }],
      _isBigNumber: true,
    })
  })
})

describe('ofItfNormalized', () => {
  it('returns normalized quint expressions', () => {
    const unnormalized = `{
  a: Set(3, 2, 1),
  b: { c: 3, b: 2, a: 1  },
  c: Map("c" -> 3, "b" -> 2, "a" -> 1)
}`

    const normalized = `{
  a: Set(1, 2, 3),
  b: { a: 1, b: 2, c: 3 },
  c: Map("a" -> 1, "b" -> 2, "c" -> 3)
}`

    const unnormalizedTrace = [buildExpression(unnormalized)]
    const normalizedTrace = [buildExpression(normalized)]
    const unnormalizedItf = toItf(['a', 'b', 'c'], unnormalizedTrace).unwrap()
    const normalizedExp = ofItfNormalized(unnormalizedItf)
    assert(zip(normalizedExp, normalizedTrace).every(([a, b]) => quintExAreEqual(a, b)))
  })
})
