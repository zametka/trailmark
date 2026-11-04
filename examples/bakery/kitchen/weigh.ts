// A loaf is weighed before it is wrapped (RULE-1).
// The prover temperature is logged at the start of the mix (RULE-23).
export function weigh(grams: number): boolean {
  return grams > 0;
}
