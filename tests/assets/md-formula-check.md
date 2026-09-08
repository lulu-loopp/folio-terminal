# Markdown 公式验收语料

用法：在被测窗口的 files 列里双击这份文件，它会开在预览 pane 上。
每个 `$$` 块应替换为渲染图，行内 `$…$` 应在句子里就地成图，
负向用例必须保持原样文字。

## 1. 行内公式

能量 $E = mc^2$，动量 $p = mv$，其中 $c$ 是光速。

勾股定理说 $a^2 + b^2 = c^2$，而 $\sqrt{2}$ 不是有理数。

中文习惯不加空格：能量$E$的值与质量$m$成正比。

一行两处：$\alpha + \beta$ 与 $\gamma \cdot \delta$。

## 2. 块级公式，单行

$$E = mc^2$$

$$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

$$\int_{0}^{\infty} e^{-x^2}\, dx = \frac{\sqrt{\pi}}{2}$$

## 3. 块级公式，多行环境

$$
\begin{aligned}
(a+b)^2 &= a^2 + 2ab + b^2 \\
(a-b)^2 &= a^2 - 2ab + b^2 \\
a^2 - b^2 &= (a+b)(a-b)
\end{aligned}
$$

$$
\begin{pmatrix}
a & b \\
c & d
\end{pmatrix}
$$

## 4. 公式住在别的块里

### 标题里的 $\Sigma$ 求和号

- 列表项里的 $y = kx + b$
- 第二项，没有公式

> 引用里的 $\nabla \times \mathbf{B}$

| 符号 | 含义 |
|---|---|
| $\pi$ | 圆周率 |
| $e$ | 自然对数底 |

## 5. 很宽的块（应在自己的块里横向滚动）

$$\forall \varepsilon > 0\, \exists \delta > 0 : |x - a| < \delta \Rightarrow |f(x) - f(a)| < \varepsilon \Rightarrow \lim_{x \to a} f(x) = f(a)$$

# 负向用例：以下每一行都应保持为字面文字

## 6. 代码里的美元符号不是公式

行内代码：`echo $HOME` 和 `$x$` 都是代码。

```bash
export PATH=$HOME/bin:$PATH
$$x^2 + y^2$$
echo "总价 $5"
```

## 7. 转义的美元符号

这件 \$5 那件 \$10。

## 8. 价格不是方程

这件 $5 那件 $10 一共 $15。

Cost $5+$10 today.

## 9. 落单的美元符号

echo $PATH 里没有配对的第二个。

$ x $ 开头带空格，不是公式。
