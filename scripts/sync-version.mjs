// Copies the version from package.json (bumped by Changesets) into src-tauri/Cargo.toml.
// tauri.conf.json already reads its version from package.json.
import { readFileSync, writeFileSync } from 'node:fs'

const { version } = JSON.parse(readFileSync('package.json', 'utf8'))
const path = 'src-tauri/Cargo.toml'
const cargo = readFileSync(path, 'utf8')
const pattern = /^version = ".*"$/m
if (!pattern.test(cargo)) throw new Error(`no package version found in ${path}`)
writeFileSync(path, cargo.replace(pattern, `version = "${version}"`))
console.log(`Cargo.toml version set to ${version}`)
