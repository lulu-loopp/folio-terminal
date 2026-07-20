# LaTeX 渲染验收语料

# 用法：在被测终端里 `Get-Content .\test-assets\latex-render-check.md`
# （或 bash: cat）。每个块级公式应替换为渲染图，负向用例必须保持原样文字。
# 规则回顾：只渲染 display 数学；分隔符多行时须独占整行；块内散文行会被拒绝。

# ========== 1. 单行 $$…$$：基础排版原语 ==========

$$E = mc^2$$

$$a^2 + b^2 = c^2$$

$$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

$$\sqrt[3]{x^3 + y^3}$$

$$f'(x) = \lim_{h \to 0} \frac{f(x+h) - f(x)}{h}$$

$$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$$

$$\int_{0}^{\infty} e^{-x^2}\, dx = \frac{\sqrt{\pi}}{2}$$

$$\prod_{k=1}^{n} k = n!$$

$$\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e$$

$$\nabla \times \mathbf{B} - \frac{1}{c}\frac{\partial \mathbf{E}}{\partial t} = \frac{4\pi}{c}\mathbf{J}$$

# ========== 2. \[…\] 形式（等价 display） ==========

\[\hat{H}\psi = E\psi\]

\[\oint_{\partial \Sigma} \mathbf{E} \cdot d\boldsymbol{\ell} = -\frac{d\Phi_B}{dt}\]

# ========== 3. 希腊字母 / 上下标 / 重音 ==========

$$\alpha\beta\gamma\delta\epsilon\zeta\eta\theta\quad \Gamma\Delta\Theta\Lambda\Xi\Pi\Sigma\Phi\Psi\Omega$$

$$x_{i}^{2} \quad a_{n+1} \quad {}^{14}_{6}\mathrm{C} \quad \vec{v} \quad \dot{x} \quad \ddot{x} \quad \bar{z} \quad \tilde{n} \quad \hat{p}$$

# ========== 4. 多行块：$$ 独占行 + 环境嵌套 ==========

$$
\begin{aligned}
(a+b)^2 &= a^2 + 2ab + b^2 \\
(a-b)^2 &= a^2 - 2ab + b^2 \\
a^2 - b^2 &= (a+b)(a-b)
\end{aligned}
$$

# ========== 5. 裸数学环境（\begin 独占行） ==========

\begin{align}
\nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_0} \\
\nabla \cdot \mathbf{B} &= 0 \\
\nabla \times \mathbf{E} &= -\frac{\partial \mathbf{B}}{\partial t}
\end{align}

# ========== 6. 矩阵家族 ==========

\begin{pmatrix}
a & b \\
c & d
\end{pmatrix}

\begin{bmatrix}
1 & 0 & 0 \\
0 & 1 & 0 \\
0 & 0 & 1
\end{bmatrix}

\begin{vmatrix}
a & b \\
c & d
\end{vmatrix}

# ========== 7. 分段函数 cases ==========

\begin{cases}
x, & \text{if } x \geq 0 \\
-x, & \text{if } x < 0
\end{cases}

# ========== 8. 大型分隔符 / 求和积分组合 ==========

$$\left[ \sum_{k=0}^{\infty} \frac{x^k}{k!} \right] = e^x$$

$$\binom{n}{k} = \frac{n!}{k!\,(n-k)!}$$

$$\frac{\partial^2 u}{\partial t^2} = c^2 \left( \frac{\partial^2 u}{\partial x^2} + \frac{\partial^2 u}{\partial y^2} \right)$$

# ========== 9. CJK 经反斜杠命令携带（护栏放行） ==========

$$
\underbrace{x + \cdots + x}_{n \text{ 项}} = nx
$$

$$P(\text{事件}) = \frac{\text{有利结果}}{\text{全部结果}}$$

# ========== 10. 关系/逻辑/集合符号 ==========

$$\forall \varepsilon > 0\, \exists \delta > 0 : |x - a| < \delta \Rightarrow |f(x) - f(a)| < \varepsilon$$

$$A \cup B,\quad A \cap B,\quad A \subseteq B,\quad x \in \mathbb{R},\quad \varnothing,\quad \aleph_0$$


# ================================================================
# 负向用例：以下每一行都应保持为字面文字，绝不能被渲染成公式
# ================================================================

# --- 内联 $…$ 已禁用 ---
能量 $E = mc^2$，动量 $p = mv$。

# --- \(…\) 内联已禁用 ---
勾股 \(a^2 + b^2 = c^2\) 定理。

# --- shell 变量不是数学 ---
echo $PATH
export PATH=$HOME/bin:$PATH
pid=$$

# --- 货币不是数学 ---
这件 $5 那件 $10 一共 $15

# --- 转义分隔符 ---
\$$x^2$$

# --- 单行块内嵌散文（2+ 多字母拉丁词，应被拒） ---
$$
ordinary English prose continues here
$$

# --- CJK 散文块（无反斜杠命令，应被拒） ---
$$
这是一段普通的中文说明文字
$$

# --- 代码围栏内的公式不渲染 ---
```text
$$x^2 + y^2$$
```
