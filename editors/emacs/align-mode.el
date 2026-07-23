(defvar align-mode-syntax-table
  (let ((st (make-syntax-table)))
    (modify-syntax-entry ?/ ". 124b" st)
    (modify-syntax-entry ?\n "> b" st)
    (modify-syntax-entry ?\' "\"" st)
    st))

(defvar align-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map (kbd "C-c C-r") 'align-run)
    (define-key map (kbd "C-c C-c") 'align-check)
    (define-key map (kbd "C-c C-f") 'align-fmt)
    map))

(defconst align-keywords
  '("fn" "return" "mut" "pub" "module" "import" "if" "else" "arena" "task_group" "match" "loop" "break" "template" "unsafe" "extern" "as" "out"))

(defconst align-attributes
  '("align" "layout" "link"))

(defconst align-types
  '("i8" "u8" "i16" "u16" "i32" "u32" "i64" "u64" "f32" "f64" "bool" "char" "str" "string" "array" "slice" "soa" "box" "raw" "builder" "writer" "reader" "buffer" "array_builder" "file" "rng" "regex" "regex_match" "tcp_conn" "tcp_listener" "udp_socket" "child" "Option" "Result" "Error" "Num" "Ord" "Eq" "vec2" "vec4" "vec8" "vec16" "mask2" "mask4" "mask8" "mask16"))

(defconst align-constants
  '("true" "false"))

(defconst align-builtins
  '("map" "par_map" "where" "reduce" "scan" "partition" "group_by" "agg" "sort" "sort_by_key" "chunks" "zip" "to_array" "map_into" "to_soa" "dict_encode" "sum" "min" "max" "count" "any" "all" "dot" "sum_where" "select" "fma" "abs" "sqrt" "floor" "ceil" "round" "trunc" "pow" "Some" "None" "Ok" "Err" "error" "map_err" "spawn" "wait" "print"))

(defconst align-number-regexp
  "\\_<\\(?:0[xX][[:xdigit:]_]+\\|0[oO][0-7_]+\\|0[bB][01_]+\\|[0-9][0-9_]*\\(?:\\.[0-9][0-9_]*\\)?\\(?:[eE][+-]?[0-9][0-9_]*\\)?\\)\\_>")

(defconst align-operators
  '(".." ":=" "->" "=>" "==" "!=" "<=" ">=" "&&" "||" "<<" ">>"
    "=" "+" "-" "*" "/" "%" "<" ">" "!" "&" "|" "^" "~" "?"))

(defconst align-operator-regexp
  (regexp-opt align-operators))

(defconst align-attribute-regexp
  (concat "\\_<" (regexp-opt align-attributes) "\\_>\\(?=\\s-*(\\)"))

(defconst align-font-lock-keywords
  `((,(regexp-opt align-keywords 'words) . font-lock-keyword-face)
    (,align-attribute-regexp . font-lock-preprocessor-face)
    (,(regexp-opt align-types 'words) . font-lock-type-face)
    (,(regexp-opt align-builtins 'words) . font-lock-builtin-face)
    (,(regexp-opt align-constants 'words) . font-lock-constant-face)
    (,align-number-regexp . font-lock-constant-face)
    (,align-operator-regexp . font-lock-keyword-face)))

(defun align--command (subcommand)
  "Build the command prefix for an Align SUBCOMMAND."
  (let ((proj-root (locate-dominating-file default-directory "Cargo.toml")))
    (if proj-root
        (format "cargo run -q --manifest-path %s --bin alignc -- %s "
                (shell-quote-argument (expand-file-name "Cargo.toml" proj-root))
                subcommand)
      (format "alignc %s " subcommand))))

(defun align-run ()
  "Run the current align file."
  (interactive)
  (compile (format "%s%s" (align--command "run")
                   (shell-quote-argument buffer-file-name))))

(defun align-check ()
  "Check (lint) the current align file."
  (interactive)
  (compile (format "%s%s" (align--command "check")
                   (shell-quote-argument buffer-file-name))))

(defun align-fmt ()
  "Format the current align file."
  (interactive)
  (when (buffer-modified-p)
    (save-buffer))
  (shell-command (format "%s%s --write" (align--command "fmt")
                         (shell-quote-argument buffer-file-name)))
  (revert-buffer t t t))

;;;###autoload
(define-derived-mode align-mode prog-mode "Align"
  "Major mode for editing Align files."
  :syntax-table align-mode-syntax-table
  (setq font-lock-defaults '(align-font-lock-keywords))
  (setq-local comment-start "// ")
  (use-local-map align-mode-map))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.align\\'" . align-mode))

(provide 'align-mode)
