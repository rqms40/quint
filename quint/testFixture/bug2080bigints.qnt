// Regression fixture for ITF integers around the json-bigint 15-character cutoff.
module bigints {
  var x: int
  action step = x' = x

  action init15m1 = x' = 10^15 - 1
  action init15 = x' = 10^15
  action init53 = x' = 2^53 - 1
  action init63 = x' = 2^63
  action initNeg15m1 = x' = -(10^15 - 1)
  action initNeg15 = x' = -(10^15)
  action initNeg14 = x' = -(10^14)
  action initNeg14m1 = x' = -(10^14 - 1)
  action initNeg53 = x' = -(2^53 - 1)
}
