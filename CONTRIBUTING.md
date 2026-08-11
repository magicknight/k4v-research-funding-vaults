# Contributing

Bug reports, adversarial fixtures, independent reproductions, and alternative
Solana designs are welcome.

Before opening a pull request:

~~~sh
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_*.py' -v
npm run check
sha256sum -c SHA256SUMS
~~~

New release paths must include both an allow case and a rejection case.
Changes that weaken a cliff, rate cap, purpose gate, market-capacity cap, or
authority invariant must be proposed as a new policy version and cannot be
described as a backward-compatible fix.
