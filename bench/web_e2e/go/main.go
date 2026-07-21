// The W7 external control: the same plaintext response from Go, driven by the SAME load generator
// as the Align servers (`EXTERNAL=host:port`). Fiber is the reference pkg.web was designed against;
// net/http is the stdlib floor for Go itself. Which one this binary is depends on the build tag —
// see run.sh, which falls back to net/http when the Fiber module cannot be fetched.
package main

import (
	"flag"
	"fmt"
	"net/http"
	"runtime"
)

func main() {
	port := flag.Int("port", 0, "listen port")
	workers := flag.Int("workers", 0, "GOMAXPROCS (0 = all cores)")
	flag.Parse()
	if *workers > 0 {
		runtime.GOMAXPROCS(*workers)
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/plaintext", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		fmt.Fprint(w, "Hello, World!")
	})
	mux.HandleFunc("/json", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, `{"message":"Hello, World!"}`)
	})
	srv := &http.Server{Addr: fmt.Sprintf("127.0.0.1:%d", *port), Handler: mux}
	if err := srv.ListenAndServe(); err != nil {
		panic(err)
	}
}
