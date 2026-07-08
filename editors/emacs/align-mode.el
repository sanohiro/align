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
  '("fn" "return" "mut" "pub" "module" "import" "if" "else" "arena" "box" "task_group" "match" "template" "unsafe" "extern" "as" "out"))

(defconst align-types
  '("i8" "u8" "i16" "u16" "i32" "u32" "i64" "u64" "f32" "f64" "bool" "char" "str" "string" "array" "slice" "raw" "writer" "reader" "buffer" "rng" "tcp_conn" "tcp_listener" "udp_socket" "child" "soa" "Option" "Result" "vec2" "vec4" "vec8" "vec16" "mask2" "mask4" "mask8" "mask16"))

(defconst align-constants
  '("true" "false"))

(defconst align-builtins
  '("map" "par_map" "where" "reduce" "scan" "partition" "group_by" "sort" "sort_by_key" "chunks" "sum" "min" "max" "count" "any" "all" "dot"))

(defconst align-font-lock-keywords
  `((,(regexp-opt align-keywords 'words) . font-lock-keyword-face)
    (,(regexp-opt align-types 'words) . font-lock-type-face)
    (,(regexp-opt align-builtins 'words) . font-lock-builtin-face)
    (,(regexp-opt align-constants 'words) . font-lock-constant-face)))

(defun align-run ()
  "Run the current align file."
  (interactive)
  (let* ((proj-root (locate-dominating-file default-directory "Cargo.toml"))
         (cargo-cmd (if proj-root 
                        (format "cargo run -q --manifest-path %sCargo.toml --bin alignc -- run " proj-root)
                      "cargo run -q --bin alignc -- run ")))
    (compile (format "%s%s" cargo-cmd (shell-quote-argument buffer-file-name)))))

(defun align-check ()
  "Check (lint) the current align file."
  (interactive)
  (let* ((proj-root (locate-dominating-file default-directory "Cargo.toml"))
         (cargo-cmd (if proj-root 
                        (format "cargo run -q --manifest-path %sCargo.toml --bin alignc -- check " proj-root)
                      "cargo run -q --bin alignc -- check ")))
    (compile (format "%s%s" cargo-cmd (shell-quote-argument buffer-file-name)))))

(defun align-fmt ()
  "Format the current align file."
  (interactive)
  (let* ((proj-root (locate-dominating-file default-directory "Cargo.toml"))
         (cargo-cmd (if proj-root 
                        (format "cargo run -q --manifest-path %sCargo.toml --bin alignc -- fmt " proj-root)
                      "cargo run -q --bin alignc -- fmt ")))
    (shell-command (format "%s%s --write" cargo-cmd (shell-quote-argument buffer-file-name)))
    (revert-buffer t t t)))

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
