# dlua

## 简介

dlua 是一个针对 lua 的预处理器，可在完全兼容原 lua 项目的情况下，为 lua 项目带来宏的能力。

所有 dlua 的功能都在注释中通过特殊关键词绑定，因此不会导致任何解析器、格式化工具失效，可以放心引入。

## 已实现的功能

### 宏变量

可以在定义变量时添加宏标记 `-- @macro`，以标注该变量为宏变量。支持局部的宏变量。

```lua
-- @macro
ITER = 1000
local a = 1
for i = 1, ITER do
	a = a + 1
end
do
	-- @macro
	ITER = 10000
	for i = 1, ITER do
		a = a + 1
	end
end
```

解析为：

```lua
local a = 1
for i = 1, 1000 do
	a = a + 1
end
do
	for i = 1, 10000 do
		a = a + 1
	end
end
```

### 宏函数

允许在定义函数时添加宏标记 `-- @macro`，以标注该函数为宏函数。也支持局部的宏函数。

```lua
-- @macro
function inc(x)
	x = x + 1
end
local a = 1
for i = 1, 1000 do
	inc(a)
end
do
	-- @macro
	local function inc(x)
		x = x + 2
	end
	for i = 1, 1000 do
		inc(a)
	end
end
```

解析为：

```lua
local a = 1
for i = 1, 1000 do
	a = a + 1
end
do
	for i = 1, 1000 do
		a = a + 2
	end
end
```


### require 支持

可以在一个文件中定义全局的宏变量或宏函数。当这个文件被其它文件的任意作用域 `require` 时，该文件中所有的全局宏变量/宏函数都将被导入 `require` 它的文件中。

```lua
-- macros.lua
-- @macro
function ADDSELF(x, value)
	x = x + value
end
-- @macro
ITER = 1000
```

```lua
-- main.lua
require("macros")
local x = 0
for i = 1, ITER do
	ADDSELF(x, i)
end
```

这两个文件将被解析为：

```lua
-- macros.lua
```

```lua
-- main.lua
require("macros")
local x = 0
for i = 1, 1000 do
	x = x + i
end
```

## 注意事项

### 复杂的宏函数

需要注意的是，本质上宏函数也只做简单的文本替换，需要警惕宏函数中使用 `local` 定义变量导致的变量污染。

特别地，如果宏函数中拥有 `return` 语句，则宏函数内只允许包含单行 `return` 语句，不允许有其它语句。

下面举出错误的使用例子：

```lua
-- @macro
function local_trash(y)
	local x = 1
	y = x + 1
end

-- @macro
function return_macro_function_not_expected()
	local a = 1
	local b = 2
	return a + b
end

local x = 2
local_trash(x)
x = x + return_macro_function_not_expected()
```

解析为：

```lua
local x = 2
local x = 1
x = x + 1
x = x + local a = 1
local b = 2
a + b
```

这显然是不合法的！

合理的用法如：

```lua
-- @macro
function V_ADD(x1, y1, x2, y2)
	return x1 + x2, y1 + y2
end
local x, y = V_ADD(1, 2, 3, 4)
```

解析为：

```lua
local x, y = 1 + 3, 2 + 4
```

### 宏嵌套

简单起见，没有实现宏嵌套功能。