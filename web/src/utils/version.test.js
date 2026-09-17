import assert from 'node:assert/strict'
import test from 'node:test'

import {nextPatchVersion, nextVersionForHistory} from './version.js'

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

test('uses the next normal version when the latest label is incrementable', () => {
  assert.equal(nextVersionForHistory(['v7.7.749', 'v7.7.748']), 'v7.7.750')
})

test('starts a safe numeric channel after a legacy date label', () => {
  assert.equal(nextVersionForHistory(['-2026.9.11-', '----2026.9.11----']), 'v0.0.1')
})

test('skips numeric labels that already exist in legacy history', () => {
  assert.equal(nextVersionForHistory(['-2026.9.11-', 'v0.0.1']), 'v0.0.2')
})

test('starts at v0.0.1 for an empty history', () => {
  assert.equal(nextVersionForHistory([]), 'v0.0.1')
})
