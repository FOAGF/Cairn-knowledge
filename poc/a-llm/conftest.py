import os, pytest, mock_server
os.environ["LITELLM_LOCAL_MODEL_COST_MAP"] = "True"   # no network fetch of the cost map
os.environ["LITELLM_LOG"] = "ERROR"
for k in list(os.environ):  # never let real keys leak into tests
    if k.startswith(("AZURE_", "OPENAI_", "ANTHROPIC_")): del os.environ[k]

@pytest.fixture(scope="session")
def _srv():
    srv, state, url = mock_server.start(); yield state, url; srv.shutdown()

@pytest.fixture
def mock(_srv):
    state, url = _srv; state.reset(); state.url = url; return state
