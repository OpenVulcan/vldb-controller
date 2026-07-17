$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$goRoot = Resolve-Path (Join-Path $repoRoot "client-go")
$protoRoot = Join-Path $repoRoot "proto"
$protoFile = Join-Path $repoRoot "proto\v1\controller.proto"

protoc `
  --proto_path=$protoRoot `
  --go_out=$goRoot `
  --go_opt=paths=source_relative `
  --go-grpc_out=$goRoot `
  --go-grpc_opt=paths=source_relative `
  $protoFile
