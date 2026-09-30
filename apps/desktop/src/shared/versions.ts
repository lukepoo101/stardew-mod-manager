/**
 * Numeric-aware comparison of dotted mod versions ("1.10" is after "1.2").
 * Missing parts count as zero, so "1.0" equals "1.0.0"; parts that are not
 * numbers compare as text. Returns -1, 0 or 1.
 */
export function compareVersions(a: string, b: string): number {
  const left = a.split(/[.+-]/);
  const right = b.split(/[.+-]/);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const x = left[index] ?? "0";
    const y = right[index] ?? "0";
    const nx = Number(x);
    const ny = Number(y);
    const numeric =
      x !== "" && y !== "" && !Number.isNaN(nx) && !Number.isNaN(ny);
    const order = numeric ? nx - ny : x.localeCompare(y);
    if (order !== 0) return order < 0 ? -1 : 1;
  }
  return 0;
}
