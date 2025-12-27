import { useState, useEffect } from 'react';
import { Editor } from './components/Editor';
import { CreateProjectModal } from './components/CreateProjectModal';
import { useDaemon } from './hooks/useDaemon';
import './App.css';

interface Project {
  id: number;
  name: string;
  created_at: string;
  cache_dir: string;
  style_profile_id?: number;
}

interface CreateProjectResponse {
  id: number;
}

function App() {
  const [currentProjectId, setCurrentProjectId] = useState<number | null>(null);
  const [currentProjectName, setCurrentProjectName] = useState<string>('My First Project');
  const [projects, setProjects] = useState<Project[]>([]);
  const [showCreateModal, setShowCreateModal] = useState(false);

  const projectsList = useDaemon<Project[]>('/projects', { method: 'GET' });
  const createProject = useDaemon<CreateProjectResponse>('/projects', { method: 'POST' });

  // Fetch projects and find/create "My First Project"
  useEffect(() => {
    const initializeProject = async () => {
      try {
        // Fetch all projects
        const projectsData = await projectsList.execute();
        if (projectsData) {
          setProjects(projectsData);

          // Find "My First Project"
          const myFirstProject = projectsData.find((p) => p.name === 'My First Project');
          
          if (myFirstProject) {
            setCurrentProjectId(myFirstProject.id);
            setCurrentProjectName(myFirstProject.name);
          } else {
            // Create "My First Project" if it doesn't exist
            const result = await createProject.execute({
              name: 'My First Project',
              cache_dir: '.cache',
            });
            
            if (result && result.id) {
              setCurrentProjectId(result.id);
              setCurrentProjectName('My First Project');
              // Refresh projects list
              const updatedProjects = await projectsList.execute();
              if (updatedProjects) {
                setProjects(updatedProjects);
              }
            }
          }
        } else {
          // If no projects data but no error, try creating "My First Project"
          const result = await createProject.execute({
            name: 'My First Project',
            cache_dir: '.cache',
          });
          if (result && result.id) {
            setCurrentProjectId(result.id);
            setCurrentProjectName('My First Project');
          }
        }
      } catch (error) {
        console.error('Error initializing project:', error);
        // If daemon is not running, we'll show loading state
        // User will need to start the daemon
      }
    };

    initializeProject();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleProjectSelect = async (projectId: number) => {
    const selectedProject = projects.find((p) => p.id === projectId);
    if (selectedProject) {
      setCurrentProjectId(projectId);
      setCurrentProjectName(selectedProject.name);
    }
  };

  const handleCreateProject = async (projectId: number, projectName: string) => {
    setCurrentProjectId(projectId);
    setCurrentProjectName(projectName);
    setShowCreateModal(false);
    // Refresh projects list
    const updatedProjects = await projectsList.execute();
    if (updatedProjects) {
      setProjects(updatedProjects);
    }
  };

  if (currentProjectId === null) {
    // Loading state while initializing project or error state
    const hasError = projectsList.error || createProject.error;
    return (
      <div
        style={{
          width: '100vw',
          height: '100vh',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          backgroundColor: '#1a1a1a',
          color: '#e5e5e5',
          gap: '1rem',
        }}
      >
        {hasError ? (
          <>
            <div style={{ fontSize: '1.25rem', fontWeight: 500 }}>
              Cannot connect to daemon
            </div>
            <div style={{ fontSize: '0.875rem', color: '#a0a0a0', textAlign: 'center', maxWidth: '400px' }}>
              Please make sure the daemon is running. Start it with:
              <br />
              <code style={{ backgroundColor: '#2a2a2a', padding: '0.25rem 0.5rem', borderRadius: '4px', marginTop: '0.5rem', display: 'inline-block' }}>
                cargo run --bin daemon
              </code>
            </div>
          </>
        ) : (
          <div>Loading...</div>
        )}
      </div>
    );
  }

  return (
    <div style={{ width: '100vw', height: '100vh', overflow: 'hidden', backgroundColor: '#1a1a1a' }}>
      <Editor
        projectId={currentProjectId}
        currentProjectName={currentProjectName}
        projects={projects}
        onProjectSelect={handleProjectSelect}
        onCreateProject={() => setShowCreateModal(true)}
      />
      {showCreateModal && (
        <CreateProjectModal
          onClose={() => setShowCreateModal(false)}
          onCreated={handleCreateProject}
        />
      )}
    </div>
  );
}

export default App;
