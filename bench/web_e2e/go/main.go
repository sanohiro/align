// The W7 external control: the same plaintext/JSON responses from Go, driven by the SAME load
// generator as the Align servers (`EXTERNAL=host:port`).
//
// Fiber is the reference `pkg.web` was designed against (`docs/impl/15-pkg-web-plan.md`), so it is
// the default. `-stdlib` switches to Go's `net/http` in the same binary, because "5.9x net/http"
// and "Nx Fiber" are different claims and both are worth being able to make.
package main

import (
	"flag"
	"fmt"
	"net/http"
	"runtime"

	"github.com/gofiber/fiber/v2"
)

func main() {
	port := flag.Int("port", 0, "listen port")
	workers := flag.Int("workers", 0, "GOMAXPROCS (0 = all cores)")
	stdlib := flag.Bool("stdlib", false, "serve with net/http instead of Fiber")
	prefork := flag.Bool("prefork", false, "Fiber prefork: one process per core, SO_REUSEPORT — the direct analogue of pkg.web workers")
	flag.Parse()
	if *workers > 0 {
		runtime.GOMAXPROCS(*workers)
	}
	addr := fmt.Sprintf("127.0.0.1:%d", *port)

	if *stdlib {
		mux := http.NewServeMux()
		mux.HandleFunc("/plaintext", func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "text/plain; charset=utf-8")
			fmt.Fprint(w, "Hello, World!")
		})
		mux.HandleFunc("/json", func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "application/json")
			fmt.Fprint(w, `{"message":"Hello, World!"}`)
		})
		panic(http.ListenAndServe(addr, mux))
	}

	// Three routes, matching the Align table shape (a static, a JSON, and a :param route).
	app := fiber.New(fiber.Config{DisableStartupMessage: true, Prefork: *prefork})
	app.Get("/plaintext", func(c *fiber.Ctx) error {
		c.Set("Content-Type", "text/plain; charset=utf-8")
		return c.SendString("Hello, World!")
	})
	app.Get("/json", func(c *fiber.Ctx) error {
		c.Set("Content-Type", "application/json")
		return c.SendString(`{"message":"Hello, World!"}`)
	})
	app.Get("/item/:id", func(c *fiber.Ctx) error {
		return c.SendString(c.Params("id"))
	})
	panic(app.Listen(addr))
}
