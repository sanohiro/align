if exists("b:did_ftplugin")
  finish
endif
let b:did_ftplugin = 1

setlocal commentstring=//\ %s
setlocal errorformat=%f:%l:%c:\ %trror:\ %m,%f:%l:%c:\ %tarning:\ %m,%f:%l:%c:\ %m
setlocal omnifunc=syntaxcomplete#Complete

let b:align_cargo_toml = findfile('Cargo.toml', expand('%:p:h') . ';')
if !empty(b:align_cargo_toml)
  let b:align_cargo_toml = fnamemodify(b:align_cargo_toml, ':p')
endif

function! s:alignc(subcommand) abort
  if !empty(get(b:, 'align_cargo_toml', ''))
    return 'cargo run -q --manifest-path ' . shellescape(b:align_cargo_toml)
          \ . ' --bin alignc -- ' . a:subcommand
  endif
  return 'alignc ' . a:subcommand
endfunction

let &l:makeprg = s:alignc('check') . ' ' . shellescape(expand('%:p'))

command! -buffer AlignFmt update | let b:winview = winsaveview() | execute 'silent !' . s:alignc('fmt') . ' ' . shellescape(expand('%:p')) . ' --write' | edit! | redraw! | call winrestview(b:winview)
command! -buffer AlignCheck execute 'make'
command! -buffer AlignRun execute '!' . s:alignc('run') . ' ' . shellescape(expand('%:p'))

let b:undo_ftplugin = 'setlocal commentstring< errorformat< makeprg< omnifunc<'
      \ . ' | delcommand AlignFmt | delcommand AlignCheck | delcommand AlignRun'
