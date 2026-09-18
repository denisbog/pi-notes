# pi-mdview golden sample

Intro paragraph with **bold**, *italic*, ~~strike~~, `inline code`,
a [link](https://example.com), and $x^2 + \alpha$ math.

## Lists

- first item that is long enough to wrap around the width boundary nicely
- second item
  - nested item with `code`
  - another nested
- [x] done task
- [ ] open task

1. ordered one
2. ordered two
   1. nested ordered
   2. nested ordered 2

> A quote with **bold** text and enough words to wrap at least once in the view.
>
> > nested quote line

```rust
fn main() {
    println!("hello");
}
```

```python
def greet(name: str) -> None:
    print(f"hi {name}")
```

| Name | Value | Notes |
|------|-------|-------|
| a | 1 | short |
| b | 2 | a longer note that wraps |
| c | 3 | ok |

---

Final paragraph.

$$
\sum_{i=1}^{n} i = \frac{n(n+1)}{2}
$$
