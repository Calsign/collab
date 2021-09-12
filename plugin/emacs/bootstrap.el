;; config file for testing

;; debugging
(setq debug-on-error t)
;; no splash
(setq inhibit-splash-screen t)

(let ((dir (file-name-directory load-file-name)))
  ;; load emacs plugin
  (require 'collab (concat dir "collab.el"))
  ;; find collab executable
  (setq collab-command-name (concat dir "../../target/debug/collab")))
