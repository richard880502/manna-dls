"""Keep the two Coolify resources independently deployable; no running daemon needed."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("docker"), "Docker Compose CLI required")
class DeploymentTests(unittest.TestCase):
    def config(self, directory, enabled=False):
        env = {**os.environ, "DATABASE_URL":"postgres://fixture", "SERVICE_PASSWORD_ADMINCODE":"fixture",
               "SERVICE_USER_MINIO":"fixture", "SERVICE_PASSWORD_MINIO":"fixture", "GRAPHITI_TOKEN":"fixture",
               "MEMORY_NETWORK_EXTERNAL":"true" if enabled else "false", "MEMORY_NETWORK_NAME":"manna-memory"}
        if not enabled:
            env.pop("MEMORY_NETWORK_NAME",None)
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
        self.assertNotIn("memory-link",memory["services"]["falkordb"]["networks"])
        self.assertTrue(memory["networks"]["memory-private"]["internal"])
        for service in memory["services"].values():
            self.assertFalse(service.get("ports"))
            self.assertNotIn("container_name",service)
        self.assertEqual(Path(memory["services"]["memory"]["build"]["context"]),ROOT / "memory-sidecar")
        self.assertNotIn("memory-graph",main["volumes"])

    def test_phase_one_needs_no_precreated_external_network(self):
        main = self.config(ROOT)
        self.assertFalse(main["networks"]["memory-link"].get("external",False))
        self.assertNotEqual(main["networks"]["memory-link"]["name"],"manna-memory")
