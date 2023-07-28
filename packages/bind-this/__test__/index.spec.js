const test = require('ava')
const { transform } = require('../index.js')

function minify(code) {
  return code.replace(/\n|\s/g, '')
}

test('base', t => {
  const { code } = transform('(linkUrl);')
  t.is(
    minify(code),
    '(this._c("linkUrl",this.linkUrl));'
  )
})


test('scope', t => {
  const { code } = transform('global.inject=function(item){item.linkUrl}')
  t.is(
    minify(code),
    'global.inject=function(item){item.linkUrl;};'
  )
})

test('with length', t => {
  const { code } = transform('global.inject=function(){(item.length)}')
  t.is(
    minify(code),
    'global.inject=function(){(this._c("item",this.item).length);};'
  )
})


test('with compute number', t => {
  const { code } = transform('global.inject=function(){(item[0])}')
  t.is(
    minify(code),
    'global.inject=function(){(this._c("item[0]",this.item[0]));};'
  )
})

test('with compute string', t => {
  const { code } = transform('global.inject=function(){(item["key"])}')
  t.is(
    minify(code),
    'global.inject=function(){(this._c("item.key",this.item["key"]));};'
  )
})

test('with compute ident', t => {
  const { code } = transform('global.inject=function(){(item[key])}')
  t.is(
    minify(code),
    'global.inject=function(){(this._c("item",this.item)[this._c("key",this.key)]);};'
  )
})

test('with compute ident and string', t => {
  const { code } = transform('global.inject=function(){(item[key].key2)}')
  t.is(
    minify(code),
    'global.inject=function(){(this._c("item",this.item)[this._c("key",this.key)].key2);};'
  )
})
