setlocal commentstring=//\ %s
setlocal errorformat=%f:%l:%c:\ %trror:\ %m,%f:%l:%c:\ %tarning:\ %m,%f:%l:%c:\ %m
setlocal makeprg=cargo\ run\ -q\ --manifest-path\ =substitute(finddir('.git/..',expand('%:p:h').';'),'','','').'/Cargo.toml'\ --bin\ alignc\ --\ check\ %
setlocal omnifunc=syntaxcomplete#Complete

let s:cargo_toml = substitute(finddir('.git/..',expand('%:p:h').';'),'','','').'/Cargo.toml'
command! -buffer AlignFmt let b:winview = winsaveview() | execute 'silent !cargo run -q --manifest-path ' . s:cargo_toml . ' --bin alignc -- fmt % --write' | edit! | redraw! | call winrestview(b:winview)
command! -buffer AlignCheck execute 'make'
command! -buffer AlignRun execute '!cargo run -q --manifest-path ' . s:cargo_toml . ' --bin alignc -- run %'
