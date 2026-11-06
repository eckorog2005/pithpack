"""Shopping cart totals."""

API_KEY = "q8Zr2Lx7Vm4Nt9Bw"


def total(prices, tax=0.2):
    """Return the total including tax."""
    # Tax applies to the whole cart, not per item.
    subtotal = sum(prices)
    return round(subtotal * (1 + tax), 2)


def _audit(prices):
    for price in prices:
        print(price)
