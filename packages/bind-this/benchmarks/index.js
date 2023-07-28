const Benchmark = require('benchmark')
const sourceCodes = require('./source-codes')
const { transform: babelTransform } = require('@mpxjs/webpack-plugin/lib/template-compiler/bind-this')
const { transform: swcTransform } = require('../index.js')

const suite = Benchmark.Suite()

suite
  .add('babel', function () {
    const option = sourceCodes[0]
    babelTransform(option.code, {
      ...option
    })
  })
  .add('swc', function () {
    const option = sourceCodes[0]
    swcTransform(option.code, {
      ...option
    })
  })
  .on('cycle', function (event) {
    console.log(String(event.target))
  })
  .on('complete', function () {
    console.log('Fastest is ' + this.filter('fastest').map('name'))
  })
  .run({ async: true })