import assert from 'node:assert/strict'
import test from 'node:test'

import {nextPatchVersion} from './version.js'

test('starts a new channel at v0.0.1 when no version exists', () => {
  assert.equal(nextPatchVersion(undefined), 'v0.0.1')
  assert.equal(nextPatchVersion(null), 'v0.0.1')
  assert.equal(nextPatchVersion(''), 'v0.0.1')
})

test('increments the final numeric component', () => {
  assert.equal(nextPatchVersion('v7.7.749'), 'v7.7.750')
  assert.equal(nextPatchVersion('release-009'), 'release-010')
})

test('rejects malformed existing version labels', () => {
  assert.equal(nextPatchVersion('v7.7.x'), '')
  assert.equal(nextPatchVersion('v7.7. 1'), '')
  assert.equal(nextPatchVersion(1), '')
})
