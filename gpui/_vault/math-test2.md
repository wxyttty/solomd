# 环境支持测试

multline（预期失败回退源码）：

$$
\begin{multline}
a + b + c = \\
d + e + f
\end{multline}
$$

split（预期正常）：

$$
\begin{split}
a + b &= c \\
d &= e + f
\end{split}
$$

aligned（预期正常）：

$$
\begin{aligned}
x &= 1 \\
y &= 2
\end{aligned}
$$

gather（预期正常）：

$$
\begin{gather}
a = b \\
c = d
\end{gather}
$$
