(require 'subr-x)
(require 'json)

(unless (boundp 'collab-command-name)
  (setq collab-command-name "collab"))

;; TODO send message to move cursor
(defun collab-on-point ()
  (unless (window-minibuffer-p)
    (setq-local x (+ x 1))
    ()))

(defun collab-on-change (pos end old-len)
  (unless (window-minibuffer-p)
    (let ((json (json-encode
                 `(:pos ,(- pos 1) :old_len ,old-len
                        :new_str ,(buffer-substring-no-properties pos end)))))
      (process-send-string collab-subprocess (concat json "\n")))))

(defun collab-process-filter (proc string)
  (if (string-prefix-p "Error" string)
      (progn (message (string-trim string)) (collab-mode -1))
    (let ((json (json-read-from-string string)))
      (let ((pos (- (assoc "pos" json) 1))
            (old-len (assoc "old_len" json))
            (new-str (assoc "new_str" json)))
        (delete-region pos (+ pos old-len))
        (let ((p (point)))
          (goto-char pos)
          (insert new-str)
          (goto-char (if (> p pos) (+ p (length new-str)) p)))
        ))))

(defun collab-process-sentinel (proc event)
  ())

(defun collab-make-subprocess ()
  (let
      ((path (buffer-file-name)))
    (unless (eq path nil)
      (setq-local
       collab-subprocess
       (make-process
        :name "emacs-collab-attach"
        :command (list collab-command-name "attach" "-m" "json" "-f" path)
        :filter 'collab-process-filter
        :sentinel 'collab-process-sentinel
        :noquery t)))))

(defun collab-info ()
  (interactive)
  (let
      ((path (buffer-file-name)))
    (unless (eq path nil)
      (message (string-trim
                (shell-command-to-string
                 (concat collab-command-name
                         " -r " (file-name-directory path) " info")))))))

(define-minor-mode collab-mode
  "Toggle collab mode."
  :init-value nil
  :lighter " collab"
  :keymap `((,(kbd "C-c i") . collab-info))
  :group 'collab
  (if collab-mode
      (progn
        (setq-local x 0)
        (collab-make-subprocess)
        (add-hook 'post-command-hook #'collab-on-point nil t)
        (add-hook 'after-change-functions #'collab-on-change nil t))
    (progn
      (remove-hook 'post-command-hook #'collab-on-point t)
      (remove-hook 'after-change-functions #'collab-on-change t))))
