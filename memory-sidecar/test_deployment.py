"""Keep the two Coolify resources independently deployable; no running daemon needed."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
FALKORDB = "falkordb/falkordb:v4.12.0@sha256:7927eb194df0fadf70e2deb6c20c8178743ec92aacd739c790f8a35ce5dee613"


@unittest.skipUnless(shutil.which("docker"), "Docker Compose CLI required")
class DeploymentTests(unittest.TestCase):
    def config(self, directory, enabled=False):
        env = {**os.environ, "DATABASE_URL":"postgres://fixture", "SERVICE_PASSWORD_ADMINCODE":"fixture",
               "SERVICE_USER_MINIO":"fixture", "SERVICE_PASSWORD_MINIO":"fixture", "GRAPHITI_TOKEN":"fixture",
               "SERVICE_PASSWORD_FALKORDB":"fixture-falkordb",
               "MEMORY_NETWORK_EXTERNAL":"true" if enabled else "false", "MEMORY_NETWORK_NAME":"manna-memory"}
        if not enabled:
            env.pop("MEMORY_NETWORK_NAME",None)
            if directory == ROOT:
                env.pop("GRAPHITI_TOKEN",None)
        for key in ("COMPOSE_PROJECT_NAME", "MEMORY_PROXY_URL", "INTERNAL_PORT"):
            env.pop(key, None)
        command = ["docker","compose","-f",str(directory / "docker-compose.yml"),"config","--format","json"]
        return json.loads(subprocess.check_output(command,cwd=directory,env=env,text=True))

    def test_resources_share_only_the_explicit_network_and_have_no_cross_build_dependency(self):
        main = self.config(ROOT, True)
        memory = self.config(ROOT / "memory-sidecar")
        self.assertEqual(set(main["services"]), {"app","minio"})
        self.assertEqual(set(memory["services"]), {"memory","falkordb"})
        for config in (main,memory):
            self.assertEqual(config["networks"]["memory-link"]["name"],"manna-memory")
            self.assertTrue(config["networks"]["memory-link"]["external"])
        self.assertIn("manna-api",main["services"]["app"]["networks"]["memory-link"]["aliases"])
        self.assertIn("manna-memory",memory["services"]["memory"]["networks"]["memory-link"]["aliases"])
        self.assertTrue(memory["networks"]["memory-private"]["internal"])
        for service in memory["services"].values():
            self.assertFalse(service.get("ports"))
            self.assertNotIn("container_name",service)
        self.assertEqual(Path(memory["services"]["memory"]["build"]["context"]),ROOT / "memory-sidecar")
        self.assertNotIn("memory-graph",main["volumes"])

    def test_falkordb_is_only_on_the_private_network_hardened_and_persisted(self):
        memory = self.config(ROOT / "memory-sidecar")
        falkordb = memory["services"]["falkordb"]
        # network_mode keeps Coolify from adding its per-resource network to this service.
        self.assertEqual(falkordb["network_mode"], f"{memory['name']}_memory-private")
        self.assertNotIn("networks", falkordb)
        self.assertEqual(falkordb["hostname"], "falkordb")
        self.assertEqual(falkordb["image"], FALKORDB)
        env = falkordb["environment"]
        self.assertEqual(env["BROWSER"], "0")
        self.assertEqual(env["REDIS_ARGS"], "--requirepass fixture-falkordb")
        self.assertEqual(env["REDISCLI_AUTH"], "fixture-falkordb")
        args = env["FALKORDB_ARGS"].split()
        for name, value in (("TIMEOUT", "1000"), ("MAX_QUEUED_QUERIES", "25"), ("THREAD_COUNT", "2")):
            self.assertEqual(args[args.index(name) + 1], value)
        self.assertEqual([v["target"] for v in falkordb["volumes"]], ["/var/lib/falkordb/data"])
        sidecar = memory["services"]["memory"]["environment"]
        self.assertEqual(sidecar["FALKORDB_PASSWORD"], "fixture-falkordb")
        self.assertEqual(sidecar["MEMORY_PROXY_URL"], "http://manna-api:8081/api/internal/memory/v1")

    def test_the_forwarder_port_is_internal_and_never_published(self):
        main = self.config(ROOT, True)
        app = main["services"]["app"]
        self.assertEqual(app["environment"]["INTERNAL_PORT"], "8081")
        self.assertEqual(app["environment"]["MEMORY_PROXY_URL"], "http://manna-api:8081/api/internal/memory/v1")
        published = [(p.get("published"), p["target"]) for p in app.get("ports", [])]
        self.assertEqual(published, [("8080", 8080)])
        self.assertNotIn(8081, [p["target"] for p in app.get("ports", [])])

    def test_phase_one_needs_no_precreated_external_network(self):
        main = self.config(ROOT)
        self.assertFalse(main["networks"]["memory-link"].get("external",False))
        self.assertNotEqual(main["networks"]["memory-link"]["name"],"manna-memory")
        self.assertEqual(main["services"]["app"]["environment"]["GRAPHITI_URL"], "")
