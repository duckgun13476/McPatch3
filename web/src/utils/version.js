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

export const hasVersionWhitespace = (label) => /\s/.test(label)
