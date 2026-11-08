//! Ready-to-run sample projects, one per runtime: a small web app that
//! answers on the app's port. They use each language's standard library only
//! (Lua needs luasocket, installed by the dependency step), so they build and
//! run without network access to package registries.

use std::path::Path;

#[derive(Clone)]
pub struct Sample {
    pub files: Vec<(String, String)>,
    /// Entry point as the managed app runs it.
    pub entry: String,
    /// Run the dependency / build step before the first start.
    pub build: bool,
}

/// Lower-case letters, digits and dashes: a valid crate / module / project name.
pub fn project_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').to_string();
    let out = if out.is_empty() || !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        format!("app-{}", out)
    } else {
        out
    };
    out.trim_end_matches('-').chars().take(40).collect()
}

/// The page every sample serves.
fn page(lang: &str) -> String {
    format!("Hello from {lang} on LinxPanel!")
}

/// `dotnet_major`: the installed SDK's major version (for TargetFramework).
pub fn sample(
    runtime: &str,
    name: &str,
    port: u16,
    dotnet_major: Option<u32>,
) -> Result<Sample, String> {
    let project = project_name(name);
    let s = |a: &str, b: String| (a.to_string(), b);
    Ok(match runtime {
        "node" => Sample {
            files: vec![
                s("package.json", format!(
                    "{{\n  \"name\": \"{project}\",\n  \"version\": \"1.0.0\",\n  \"private\": true,\n  \"scripts\": {{ \"start\": \"node server.js\" }}\n}}\n"
                )),
                s("server.js", format!(
                    "// Sample created by LinxPanel. The panel sets PORT from the app's settings.\n\
                     const http = require('http')\n\n\
                     const port = Number(process.env.PORT) || {port}\n\
                     http.createServer((req, res) => {{\n  \
                       res.writeHead(200, {{ 'Content-Type': 'text/plain; charset=utf-8' }})\n  \
                       res.end('{text}\\n')\n\
                     }}).listen(port, '0.0.0.0', () => console.log(`Listening on :${{port}}`))\n",
                    text = page("Node.js")
                )),
            ],
            entry: "server.js".into(),
            build: false,
        },
        "python" => Sample {
            files: vec![s("app.py", format!(
                "# Sample created by LinxPanel. The panel sets PORT from the app's settings.\n\
                 import os\nfrom http.server import BaseHTTPRequestHandler, ThreadingHTTPServer\n\n\n\
                 class Handler(BaseHTTPRequestHandler):\n    \
                     def do_GET(self):\n        \
                         body = \"{text}\\n\".encode()\n        \
                         self.send_response(200)\n        \
                         self.send_header(\"Content-Type\", \"text/plain; charset=utf-8\")\n        \
                         self.send_header(\"Content-Length\", str(len(body)))\n        \
                         self.end_headers()\n        \
                         self.wfile.write(body)\n\n\n\
                 port = int(os.environ.get(\"PORT\", \"{port}\"))\n\
                 print(f\"Listening on :{{port}}\", flush=True)\n\
                 ThreadingHTTPServer((\"0.0.0.0\", port), Handler).serve_forever()\n",
                text = page("Python")
            ))],
            entry: "app.py".into(),
            build: false,
        },
        "php" => Sample {
            files: vec![s("index.php", format!(
                "<?php\n// Sample created by LinxPanel, served by PHP's built-in server.\n\
                 // For production websites, create a PHP site on the Sites page instead.\n\
                 header('Content-Type: text/plain; charset=utf-8');\n\
                 echo \"{text}\\n\";\necho 'PHP ' . PHP_VERSION . \"\\n\";\n",
                text = page("PHP")
            ))],
            entry: format!("-S 0.0.0.0:{} index.php", port),
            build: false,
        },
        "go" => Sample {
            files: vec![
                s("go.mod", format!("module {project}\n\ngo 1.21\n")),
                s("main.go", format!(
                    "// Sample created by LinxPanel. The panel sets PORT from the app's settings.\n\
                     package main\n\nimport (\n\t\"fmt\"\n\t\"log\"\n\t\"net/http\"\n\t\"os\"\n)\n\n\
                     func main() {{\n\
                     \tport := os.Getenv(\"PORT\")\n\
                     \tif port == \"\" {{\n\t\tport = \"{port}\"\n\t}}\n\
                     \thttp.HandleFunc(\"/\", func(w http.ResponseWriter, r *http.Request) {{\n\
                     \t\tfmt.Fprintln(w, \"{text}\")\n\t}})\n\
                     \tlog.Printf(\"Listening on :%s\", port)\n\
                     \tlog.Fatal(http.ListenAndServe(\":\"+port, nil))\n}}\n",
                    text = page("Go")
                )),
            ],
            // Source: the panel compiles it with `go build` when it starts.
            entry: ".".into(),
            build: false,
        },
        "java" => Sample {
            files: vec![s("Main.java", format!(
                "// Sample created by LinxPanel; runs straight from source (Java 11+).\n\
                 // The panel sets PORT from the app's settings.\n\
                 import com.sun.net.httpserver.HttpServer;\n\
                 import java.io.OutputStream;\nimport java.net.InetSocketAddress;\n\
                 import java.nio.charset.StandardCharsets;\n\n\
                 public class Main {{\n    \
                     public static void main(String[] args) throws Exception {{\n        \
                         int port = Integer.parseInt(System.getenv().getOrDefault(\"PORT\", \"{port}\"));\n        \
                         HttpServer server = HttpServer.create(new InetSocketAddress(port), 0);\n        \
                         server.createContext(\"/\", exchange -> {{\n            \
                             byte[] body = \"{text}\\n\".getBytes(StandardCharsets.UTF_8);\n            \
                             exchange.getResponseHeaders().set(\"Content-Type\", \"text/plain; charset=utf-8\");\n            \
                             exchange.sendResponseHeaders(200, body.length);\n            \
                             try (OutputStream out = exchange.getResponseBody()) {{ out.write(body); }}\n        \
                         }});\n        \
                         server.start();\n        \
                         System.out.println(\"Listening on :\" + port);\n    \
                     }}\n}}\n",
                text = page("Java")
            ))],
            entry: "Main.java".into(),
            build: false,
        },
        "dotnet" => {
            let major = dotnet_major.ok_or("The .NET SDK is not installed")?;
            let class = project.replace('-', "_");
            Sample {
                files: vec![
                    s(&format!("{class}.csproj"), format!(
                        "<Project Sdk=\"Microsoft.NET.Sdk.Web\">\n  <PropertyGroup>\n    \
                         <TargetFramework>net{major}.0</TargetFramework>\n    \
                         <Nullable>enable</Nullable>\n    <ImplicitUsings>enable</ImplicitUsings>\n    \
                         <InvariantGlobalization>true</InvariantGlobalization>\n  </PropertyGroup>\n</Project>\n"
                    )),
                    s("Program.cs", format!(
                        "// Sample created by LinxPanel. The panel sets ASPNETCORE_URLS from the app's port.\n\
                         var builder = WebApplication.CreateBuilder(args);\n\
                         var app = builder.Build();\n\
                         app.MapGet(\"/\", () => \"{text}\");\n\
                         app.Run();\n",
                        text = page("C# / .NET")
                    )),
                ],
                entry: format!("publish/{class}.dll"),
                build: true,
            }
        }
        "rust" => Sample {
            files: vec![
                s("Cargo.toml", format!(
                    "[package]\nname = \"{project}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"
                )),
                s("src/main.rs", format!(
                    "//! Sample created by LinxPanel: a tiny HTTP server on the standard library.\n\
                     //! The panel sets PORT from the app's settings.\n\
                     use std::io::{{Read, Write}};\nuse std::net::TcpListener;\n\n\
                     fn main() {{\n    \
                         let port = std::env::var(\"PORT\").unwrap_or_else(|_| \"{port}\".into());\n    \
                         let listener = TcpListener::bind(format!(\"0.0.0.0:{{port}}\")).expect(\"bind\");\n    \
                         println!(\"Listening on :{{port}}\");\n    \
                         for stream in listener.incoming().flatten() {{\n        \
                             std::thread::spawn(move || {{\n            \
                                 let mut stream = stream;\n            \
                                 let mut buf = [0u8; 4096];\n            \
                                 let _ = stream.read(&mut buf);\n            \
                                 let body = \"{text}\\n\";\n            \
                                 let _ = write!(stream, \"HTTP/1.1 200 OK\\r\\nContent-Type: text/plain; charset=utf-8\\r\\nContent-Length: {{}}\\r\\nConnection: close\\r\\n\\r\\n{{}}\", body.len(), body);\n        \
                             }});\n    \
                         }}\n}}\n",
                    text = page("Rust")
                )),
            ],
            entry: format!("target/release/{}", project),
            build: true,
        },
        "ruby" => Sample {
            files: vec![s("app.rb", format!(
                "# Sample created by LinxPanel: a tiny HTTP server on the standard library.\n\
                 # The panel sets PORT from the app's settings.\n\
                 require 'socket'\n\n\
                 port = Integer(ENV.fetch('PORT', '{port}'))\n\
                 server = TCPServer.new('0.0.0.0', port)\n\
                 puts \"Listening on :#{{port}}\"\n$stdout.flush\n\
                 loop do\n  \
                   Thread.start(server.accept) do |client|\n    \
                     client.readpartial(4096) rescue nil\n    \
                     body = \"{text}\\n\"\n    \
                     client.write \"HTTP/1.1 200 OK\\r\\nContent-Type: text/plain; charset=utf-8\\r\\nContent-Length: #{{body.bytesize}}\\r\\nConnection: close\\r\\n\\r\\n#{{body}}\"\n    \
                     client.close\n  \
                   end\nend\n",
                text = page("Ruby")
            ))],
            entry: "app.rb".into(),
            build: false,
        },
        "lua" => Sample {
            files: vec![
                s(&format!("{project}-dev-1.rockspec"), format!(
                    "package = \"{project}\"\nversion = \"dev-1\"\nsource = {{ url = \"file://.\" }}\n\
                     dependencies = {{ \"lua >= 5.1\", \"luasocket\" }}\n\
                     build = {{ type = \"builtin\", modules = {{}} }}\n"
                )),
                s("main.lua", format!(
                    "-- Sample created by LinxPanel: an HTTP server on luasocket\n\
                     -- (installed into ./lua_modules by the dependency step).\n\
                     -- The panel sets PORT from the app's settings.\n\
                     local socket = require(\"socket\")\n\n\
                     local port = tonumber(os.getenv(\"PORT\") or \"{port}\")\n\
                     local server = assert(socket.bind(\"0.0.0.0\", port))\n\
                     print(\"Listening on :\" .. port)\nio.stdout:flush()\n\
                     while true do\n  \
                       local client = server:accept()\n  \
                       client:settimeout(5)\n  \
                       client:receive(\"*l\")\n  \
                       local body = \"{text}\\n\"\n  \
                       client:send(\"HTTP/1.1 200 OK\\r\\nContent-Type: text/plain; charset=utf-8\\r\\nContent-Length: \" .. #body .. \"\\r\\nConnection: close\\r\\n\\r\\n\" .. body)\n  \
                       client:close()\nend\n",
                    text = page("Lua")
                )),
            ],
            entry: "main.lua".into(),
            build: true,
        },
        other => return Err(format!("No sample for {}", other)),
    })
}

/// Writes the sample into `dir` (which must be missing or empty).
pub fn write(dir: &Path, sample: &Sample) -> Result<(), String> {
    if dir.exists()
        && std::fs::read_dir(dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        return Err(format!(
            "{} is not empty; choose a new or empty directory for the sample",
            dir.display()
        ));
    }
    super::fileops::create_dirs_like_parent(dir)
        .map_err(|e| format!("{}: {}", dir.display(), e))?;
    for (name, content) in &sample.files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, content).map_err(|e| format!("{}: {}", path.display(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names() {
        assert_eq!(project_name("My Demo App"), "my-demo-app");
        assert_eq!(project_name("  测试 "), "app");
        assert_eq!(project_name("9lives"), "app-9lives");
        assert_eq!(project_name("a--b__c"), "a-b-c");
    }

    #[test]
    fn every_runtime_has_a_sample() {
        for rt in [
            "node", "python", "php", "go", "java", "dotnet", "rust", "ruby", "lua",
        ] {
            let s = sample(rt, "Demo", 18000, Some(10)).unwrap();
            assert!(!s.files.is_empty() && !s.entry.is_empty(), "{rt}");
            // Some file serves the greeting.
            assert!(
                s.files.iter().any(|(_, c)| c.contains("on LinxPanel!")),
                "{rt}"
            );
        }
        assert!(sample("dotnet", "x", 1, None).is_err());
        assert_eq!(
            sample("rust", "Hello World", 1, None).unwrap().entry,
            "target/release/hello-world"
        );
        assert_eq!(
            sample("dotnet", "my-app", 1, Some(8)).unwrap().entry,
            "publish/my_app.dll"
        );
        assert!(sample("dotnet", "x", 1, Some(8)).unwrap().files[0]
            .1
            .contains("net8.0"));
    }
}
