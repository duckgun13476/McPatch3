export const nextPatchVersion = (label) => {
  if (label === undefined || label === null || label === '') {
    return 'v0.0.1'
  }

  if (typeof label !== 'string' || /\s/.test(label)) {
    return ''
  }

  const match = label.match(/^(.*?)(\d+)$/)
  if (!match || match[1] === '') {
    return ''
  }

  const [, prefix, patch] = match
  const nextPatch = (BigInt(patch) + 1n).toString().padStart(patch.length, '0')
  return `${prefix}${nextPatch}`
}

export const nextVersionForHistory = (labels = []) => {
  const normalizedLabels = Array.isArray(labels)
    ? labels.filter((label) => typeof label === 'string' && label !== '')
    : []
  const existingLabels = new Set(normalizedLabels)
  const incremented = nextPatchVersion(normalizedLabels[0])

  if (incremented && !existingLabels.has(incremented)) {
    return incremented
  }

  for (let patch = 1; patch <= existingLabels.size + 1; patch += 1) {
    const candidate = `v0.0.${patch}`
    if (!existingLabels.has(candidate)) {
      return candidate
    }
  }

  return ''
}

export const hasVersionWhitespace = (label) => /\s/.test(label)
