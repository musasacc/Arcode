module github.com/example/server

go 1.21

require (
	github.com/gin-gonic/gin v1.9.1
	github.com/stretchr/testify v1.8.4
	golang.org/x/net v0.20.0 // indirect
)

require gopkg.in/yaml.v3 v3.0.1 // indirect

replace github.com/example/shared => ../shared
